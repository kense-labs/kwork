//! Studio Hub backend & domain models connecting KUI's 235 components and 46 screen patterns
//! directly to `kar`'s real Rust backend capabilities:
//! - `kar::workbench_git::Repository` (branch discovery, status, staging, commit, pull/push, commit graph)
//! - `kar::inventory` (`Runtime::skills`, `mcp_servers`, `commands`, `tools`, plus local `SKILL.md` & built-in MCP)
//! - Global workspace & `ChatStore` full-text search (paths, source lines, conversation messages)
//! - `UsageSnapshot` & `AccountUsageSnapshot` token/cache/quota analytics (`DailyActivity`, `ActivityRings`, `PromptCache`)
//! - Cross-runtime portable session handoff (`HandoffRequest` context packing)
//! - Structured multi-source diagnostics ring buffer (`LogStream`)

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context as _, Result};
use kar::{
    UsageSnapshot,
    chat::{ChatStore, Item, Role},
    inventory::{CommandEntry, InventoryOptions, InventoryResult, SkillScope},
    workbench_git::{BranchQuery, Repository},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::types::{ProviderStatusInfo, runtime_display_name};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Native Git Repository, BranchGraph & Workspace File Models
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFileEntry {
    pub path: PathBuf,
    pub index_status: char,
    pub worktree_status: char,
    #[serde(default)]
    pub added_lines: usize,
    #[serde(default)]
    pub deleted_lines: usize,
}

impl GitFileEntry {
    pub fn is_staged(&self) -> bool {
        !matches!(self.index_status, ' ' | '.' | '?')
    }

    pub fn has_unstaged(&self) -> bool {
        !matches!(self.worktree_status, ' ' | '.')
    }

    pub fn is_untracked(&self) -> bool {
        self.index_status == '?' && self.worktree_status == '?'
    }

    pub fn badge_char(&self) -> &'static str {
        if self.is_untracked() {
            "?"
        } else if self.index_status == 'A' || self.worktree_status == 'A' {
            "A"
        } else if self.index_status == 'D' || self.worktree_status == 'D' {
            "D"
        } else {
            "M"
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommitRecord {
    pub id: String,
    pub hash: String,
    pub message: String,
    pub body: Option<String>,
    pub branch: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub additions: u32,
    pub deletions: u32,
    pub files: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorkspaceSnapshot {
    pub is_repo: bool,
    pub repo_root: Option<PathBuf>,
    pub current_branch: Option<String>,
    pub default_branch: String,
    pub upstream: Option<String>,
    #[serde(default)]
    pub has_origin: bool,
    pub ahead: u64,
    pub behind: u64,
    pub local_branches: Vec<String>,
    pub files: Vec<GitFileEntry>,
    pub commits: Vec<GitCommitRecord>,
    pub error: Option<String>,
    pub is_demo: bool,
}

impl GitWorkspaceSnapshot {
    pub fn dirty_count(&self) -> usize {
        self.files.len()
    }

    pub fn staged_count(&self) -> usize {
        self.files.iter().filter(|f| f.is_staged()).count()
    }

    pub fn line_stats(&self) -> (usize, usize) {
        self.files.iter().fold((0, 0), |(a, d), f| {
            (a + f.added_lines, d + f.deleted_lines)
        })
    }
}

/// Discover real Git state via `kar::workbench_git::Repository` + bounded `git log` commit graph.
pub fn discover_git_workspace(workspace: &Path) -> GitWorkspaceSnapshot {
    let repo = match Repository::open(workspace) {
        Ok(r) => r,
        Err(err) => {
            return GitWorkspaceSnapshot {
                is_repo: false,
                default_branch: "main".into(),
                error: Some(err.to_string()),
                ..Default::default()
            };
        }
    };
    let root = repo.root().to_path_buf();
    let status = match repo.status() {
        Ok(s) => s,
        Err(err) => {
            return GitWorkspaceSnapshot {
                is_repo: true,
                repo_root: Some(root),
                default_branch: "main".into(),
                error: Some(err.to_string()),
                ..Default::default()
            };
        }
    };

    let branch_page = repo.branches(&BranchQuery::default()).unwrap_or_default();
    let default_branch = branch_page
        .refs
        .iter()
        .find(|b| b.is_default && !b.is_remote)
        .or_else(|| branch_page.refs.iter().find(|b| !b.is_remote && b.name == "main"))
        .map(|b| b.name.clone())
        .unwrap_or_else(|| "main".into());
    let local_branches: Vec<String> = branch_page
        .refs
        .iter()
        .filter(|b| !b.is_remote)
        .map(|b| b.name.clone())
        .collect();

    let mut numstat_map: std::collections::HashMap<PathBuf, (usize, usize)> =
        std::collections::HashMap::new();
    if let Ok(out) = Command::new("git")
        .current_dir(&root)
        .args(["diff", "--no-renames", "--numstat", "-z", "HEAD"])
        .output()
        && out.status.success()
    {
        for record in out.stdout.split(|byte| *byte == 0) {
            let mut cols = record.splitn(3, |byte| *byte == b'\t');
            if let (Some(a), Some(d), Some(p)) = (cols.next(), cols.next(), cols.next()) {
                #[cfg(unix)]
                let path = {
                    use std::os::unix::ffi::OsStrExt;
                    PathBuf::from(std::ffi::OsStr::from_bytes(p))
                };
                #[cfg(not(unix))]
                let Ok(path) = std::str::from_utf8(p).map(PathBuf::from) else {
                    continue;
                };
                let count = |n: &[u8]| std::str::from_utf8(n).ok()?.parse::<usize>().ok();
                numstat_map.insert(path, (count(a).unwrap_or(0), count(d).unwrap_or(0)));
            }
        }
    }

    let current_branch = status.branch.clone();
    let files: Vec<GitFileEntry> = status
        .files
        .into_iter()
        .map(|f| {
            let (added_lines, deleted_lines) =
                numstat_map.get(&f.path).copied().unwrap_or((0, 0));
            GitFileEntry {
                path: f.path,
                index_status: f.index,
                worktree_status: f.worktree,
                added_lines,
                deleted_lines,
            }
        })
        .collect();

    let commits = read_git_commit_graph(
        &root,
        current_branch.as_deref().unwrap_or("main"),
        &default_branch,
    );

    GitWorkspaceSnapshot {
        is_repo: true,
        has_origin: Command::new("git").current_dir(&root).args(["remote", "get-url", "origin"])
            .output().is_ok_and(|out| out.status.success()),
        repo_root: Some(root),
        current_branch,
        default_branch,
        upstream: status.upstream,
        ahead: status.ahead,
        behind: status.behind,
        local_branches,
        files,
        commits,
        error: None,
        is_demo: false,
    }
}

fn read_git_commit_graph(
    repo_root: &Path,
    current_branch: &str,
    default_branch: &str,
) -> Vec<GitCommitRecord> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "--no-optional-locks",
            "log",
            "--all",
            "--max-count=16",
            "--date=relative",
            "--format=__COMMIT__%x09%H%x09%h%x09%P%x09%an%x09%ad%x09%D%x09%s",
            "--shortstat",
        ])
        .output();
    let Ok(output) = out else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut commits: Vec<GitCommitRecord> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("__COMMIT__\t") {
            let parts: Vec<&str> = rest.splitn(7, '\t').collect();
            if parts.len() == 7 {
                let full_hash = parts[0].trim().to_owned();
                let short_hash = parts[1].trim().to_owned();
                let parents: Vec<String> = parts[2]
                    .split_whitespace()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                let author = parts[3].trim().to_owned();
                let date = parts[4].trim().to_owned();
                let refs = parts[5].trim();
                let subject = parts[6].trim().to_owned();
                let branch = infer_commit_lane(refs, current_branch, default_branch, commits.len());
                commits.push(GitCommitRecord {
                    id: full_hash,
                    hash: short_hash,
                    message: subject,
                    body: (!refs.is_empty()).then(|| refs.to_owned()),
                    branch,
                    parents,
                    author,
                    date,
                    additions: 0,
                    deletions: 0,
                    files: 0,
                });
            }
        } else if let Some(last) = commits.last_mut() {
            let trimmed = line.trim();
            if trimmed.contains("changed") {
                for seg in trimmed.split(',') {
                    let seg = seg.trim();
                    let num = seg
                        .split_whitespace()
                        .next()
                        .and_then(|n| n.parse::<u32>().ok())
                        .unwrap_or(0);
                    if seg.contains("file") {
                        last.files = num;
                    } else if seg.contains("insertion") || seg.contains("(+) ") || seg.ends_with("(+)") {
                        last.additions = num;
                    } else if seg.contains("deletion") || seg.ends_with("(-)") {
                        last.deletions = num;
                    }
                }
            }
        }
    }
    commits
}

fn infer_commit_lane(
    refs: &str,
    current_branch: &str,
    default_branch: &str,
    idx: usize,
) -> String {
    for token in refs.split(',').map(str::trim) {
        let cleaned = token.strip_prefix("HEAD -> ").unwrap_or(token);
        if cleaned == default_branch || cleaned.ends_with(&format!("/{default_branch}")) {
            return default_branch.to_owned();
        }
        if !cleaned.is_empty() && !cleaned.starts_with("tag:") && !cleaned.contains("origin/HEAD") {
            let short = cleaned.strip_prefix("origin/").unwrap_or(cleaned);
            return short.to_owned();
        }
    }
    if current_branch != default_branch && idx < 3 {
        current_branch.to_owned()
    } else {
        default_branch.to_owned()
    }
}

/// Demo Git workspace snapshot (clearly marked `is_demo = true`) for showcase mode when cwd has no multi-lane PR.
pub fn demo_git_snapshot() -> GitWorkspaceSnapshot {
    GitWorkspaceSnapshot {
        is_repo: true,
        has_origin: false,
        repo_root: Some(PathBuf::from("/workspace/kar-studio")),
        current_branch: Some("feat/offline-sync".into()),
        default_branch: "main".into(),
        upstream: Some("origin/feat/offline-sync".into()),
        ahead: 2,
        behind: 0,
        local_branches: vec![
            "main".into(),
            "feat/offline-sync".into(),
            "fix/revision-fence".into(),
        ],
        files: vec![
            GitFileEntry {
                path: PathBuf::from("src/sync/queue.ts"),
                index_status: 'M',
                worktree_status: ' ',
                added_lines: 48,
                deleted_lines: 1,
            },
            GitFileEntry {
                path: PathBuf::from("src/sync/reconcile.rs"),
                index_status: 'A',
                worktree_status: ' ',
                added_lines: 19,
                deleted_lines: 0,
            },
            GitFileEntry {
                path: PathBuf::from("docs/minutes_copy.docx"),
                index_status: ' ',
                worktree_status: 'M',
                added_lines: 4,
                deleted_lines: 1,
            },
        ],
        commits: vec![
            GitCommitRecord {
                id: "c6".into(),
                hash: "8f4a21c".into(),
                message: "feat(git): 接入 KUI BranchGraph 与工作区暂存提交".into(),
                body: Some("支持合并 / 变基 / 压缩策略实时动画预览与一键智能体审查".into()),
                branch: "feat/kui-studio-hub".into(),
                parents: vec!["c5".into()],
                author: "Leon".into(),
                date: "12 分钟前".into(),
                additions: 342,
                deletions: 28,
                files: 5,
            },
            GitCommitRecord {
                id: "m4".into(),
                hash: "4d91e0b".into(),
                message: "perf(store): 优化流式输出 4MiB 预算与详情 LRU 回收".into(),
                body: Some("保护待审批工具载荷免于自动淘汰".into()),
                branch: "main".into(),
                parents: vec!["m3".into()],
                author: "Kar Core".into(),
                date: "1 小时前".into(),
                additions: 118,
                deletions: 44,
                files: 3,
            },
            GitCommitRecord {
                id: "c5".into(),
                hash: "2c7b904".into(),
                message: "feat(skills): 打通 kar::inventory 技能与 MCP 发现中心".into(),
                body: Some("支持 $skill 与 /command 一键注入底部编辑器".into()),
                branch: "feat/kui-studio-hub".into(),
                parents: vec!["m3".into()],
                author: "Leon".into(),
                date: "2 小时前".into(),
                additions: 276,
                deletions: 19,
                files: 4,
            },
            GitCommitRecord {
                id: "r1".into(),
                hash: "9a13f6e".into(),
                message: "fix(coedit): 补充断连后 OutcomeUnknown 诚实围栏回执".into(),
                body: None,
                branch: "fix/revision-fence".into(),
                parents: vec!["m2".into()],
                author: "Kantor Team".into(),
                date: "4 小时前".into(),
                additions: 64,
                deletions: 12,
                files: 2,
            },
            GitCommitRecord {
                id: "m3".into(),
                hash: "7e50c12".into(),
                message: "feat(remote): 6 位 60 秒单次配对短码与 HMAC 双向网关".into(),
                body: None,
                branch: "main".into(),
                parents: vec!["m2".into()],
                author: "Leon".into(),
                date: "昨天".into(),
                additions: 512,
                deletions: 31,
                files: 6,
            },
            GitCommitRecord {
                id: "m2".into(),
                hash: "1b88d40".into(),
                message: "chore: 初始化 Kar + Kantor 纯 GPUI 双栏工作台基线".into(),
                body: None,
                branch: "main".into(),
                parents: Vec::new(),
                author: "Leon".into(),
                date: "2 天前".into(),
                additions: 1420,
                deletions: 0,
                files: 18,
            },
        ],
        error: None,
        is_demo: true,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunkData {
    pub start_line: usize,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFilePreview {
    pub rel_path: PathBuf,
    pub abs_path: PathBuf,
    pub language: String,
    pub content: String,
    pub total_lines: usize,
    pub truncated: bool,
    pub diff_hunks: Vec<DiffHunkData>,
}

pub fn read_workspace_file_preview(workspace: &Path, rel_path: &Path) -> Result<WorkspaceFilePreview> {
    let abs = if rel_path.is_absolute() {
        rel_path.to_path_buf()
    } else {
        workspace.join(rel_path)
    };
    let canonical_ws = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let canonical_file = abs
        .canonicalize()
        .with_context(|| format!("无法打开文件 {}", abs.display()))?;
    if !canonical_file.starts_with(&canonical_ws) {
        anyhow::bail!("拒绝读取工作区目录之外的文件");
    }
    let bytes = std::fs::read(&canonical_file)?;
    const MAX_PREVIEW_BYTES: usize = 256 * 1024;
    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    let slice = if truncated {
        &bytes[..MAX_PREVIEW_BYTES]
    } else {
        &bytes[..]
    };
    let content = String::from_utf8_lossy(slice).into_owned();
    let total_lines = content.lines().count().max(1);
    let clean_rel = canonical_file
        .strip_prefix(&canonical_ws)
        .unwrap_or(rel_path)
        .to_path_buf();
    let language = detect_language(&clean_rel);
    let diff_hunks = read_file_git_hunks(&canonical_ws, &clean_rel);

    Ok(WorkspaceFilePreview {
        rel_path: clean_rel,
        abs_path: canonical_file,
        language,
        content,
        total_lines,
        truncated,
        diff_hunks,
    })
}

fn detect_language(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "Rust",
        "toml" => "TOML",
        "md" | "markdown" => "Markdown",
        "json" | "jsonl" => "JSON",
        "yaml" | "yml" => "YAML",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" => "JavaScript",
        "py" => "Python",
        "sh" | "zsh" | "bash" => "Shell",
        "sql" => "SQL",
        "html" => "HTML",
        "css" => "CSS",
        other if !other.is_empty() => return other.to_uppercase(),
        _ => "Text",
    }
    .to_owned()
}

/// Parse unified `git diff -U0` output into 1-based hunks for KUI `PatchReview`.
pub fn parse_unified_diff_hunks(diff_text: &str) -> Vec<DiffHunkData> {
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunkData> = None;
    for line in diff_text.lines() {
        if line.starts_with("@@ ") {
            if let Some(h) = current.take()
                && (!h.before.is_empty() || !h.after.is_empty())
            {
                hunks.push(h);
            }
            // Format: @@ -old_start[,old_count] +new_start[,new_count] @@
            let new_start = line
                .split_whitespace()
                .find_map(|tok| tok.strip_prefix('+'))
                .and_then(|s| s.split(',').next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1);
            current = Some(DiffHunkData {
                start_line: new_start,
                before: Vec::new(),
                after: Vec::new(),
            });
        } else if line.starts_with("diff --git ") {
            if let Some(h) = current.take()
                && (!h.before.is_empty() || !h.after.is_empty())
            {
                hunks.push(h);
            }
        } else if let Some(ref mut h) = current {
            if let Some(removed) = line.strip_prefix('-') {
                h.before.push(removed.to_owned());
            } else if let Some(added) = line.strip_prefix('+') {
                h.after.push(added.to_owned());
            }
        }
    }
    if let Some(h) = current
        && (!h.before.is_empty() || !h.after.is_empty())
    {
        hunks.push(h);
    }
    hunks
}

fn read_file_git_hunks(workspace: &Path, rel_path: &Path) -> Vec<DiffHunkData> {
    read_single_file_diff_hunks(workspace, rel_path, false)
}

/// Read hunk-level diff for a single changed or untracked file (with demo fallback when `is_demo == true`).
pub fn read_single_file_diff_hunks(
    workspace: &Path,
    rel_path: &Path,
    is_demo: bool,
) -> Vec<DiffHunkData> {
    read_diff_side(workspace, rel_path, is_demo, GitDiffSide::Combined)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GitDiffSide {
    Combined,
    Staged,
    WorkingTree,
}

pub fn read_diff_side(
    workspace: &Path,
    rel_path: &Path,
    is_demo: bool,
    side: GitDiffSide,
) -> Vec<DiffHunkData> {
    let git_file = |args: &[&str]| {
        Command::new("git")
            .current_dir(workspace)
            .args(["--no-optional-locks", "--literal-pathspecs"])
            .args(args)
            .arg("--")
            .arg(rel_path)
            .output()
    };
    let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "-U0"];
    match side {
        GitDiffSide::Combined => args.push("HEAD"),
        GitDiffSide::Staged => args.push("--cached"),
        GitDiffSide::WorkingTree => {}
    }
    let diff = git_file(&args);
    let has_head = diff.as_ref().is_ok_and(|out| out.status.success());
    if let Ok(out) = diff
        && out.status.success()
        && !out.stdout.is_empty()
    {
        return parse_unified_diff_hunks(&String::from_utf8_lossy(&out.stdout));
    }

    // A tracked, unchanged file and a file outside a repository have no diff.
    // Before the first commit, both indexed and untracked files are additions.
    let args: &[&str] = if has_head {
        &["ls-files", "--others", "--exclude-standard"]
    } else {
        &["ls-files", "--cached", "--others", "--exclude-standard"]
    };
    let new_file = side != GitDiffSide::Staged && git_file(args).is_ok_and(|out| out.status.success() && !out.stdout.is_empty());
    let candidate = workspace.join(rel_path);
    if new_file
        && candidate.is_file()
        && let Ok(raw) = std::fs::read_to_string(&candidate)
    {
        let after: Vec<String> = raw.lines().take(32).map(str::to_owned).collect();
        if !after.is_empty() {
            return vec![DiffHunkData {
                start_line: 1,
                before: Vec::new(),
                after,
            }];
        }
    }

    if is_demo {
        return vec![DiffHunkData {
            start_line: 42,
            before: vec!["    self.active_office_tab = Some(idx);".into()],
            after: vec![
                "    self.right_surface = RightPaneSurface::Office;".into(),
                "    self.active_office_tab = Some(idx);".into(),
            ],
        }];
    }

    Vec::new()
}

/// Unstage explicit paths from the Git index (supports both normal repos and unborn initial commits).
pub fn unstage_git_paths(workspace: &Path, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let repo = Repository::open(workspace)?;
    let root = repo.root().to_path_buf();
    let has_head = Command::new("git")
        .current_dir(&root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["--no-optional-locks", "rev-parse", "--verify", "--quiet", "HEAD"])
        .status()
        .is_ok_and(|s| s.success());

    let mut cmd = Command::new("git");
    cmd.current_dir(&root).env("GIT_TERMINAL_PROMPT", "0");
    if has_head {
        cmd.args(["--no-optional-locks", "reset", "-q", "HEAD", "--"]);
    } else {
        cmd.args(["--no-optional-locks", "rm", "-q", "--cached", "-r", "--"]);
    }
    for p in paths {
        cmd.arg(p);
    }
    let out = cmd.output().context("无法调用 git 取消暂存")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        anyhow::bail!(if stderr.is_empty() {
            "git reset HEAD 执行失败".to_owned()
        } else {
            stderr
        });
    }
    Ok(())
}

/// Switch to an existing branch or create and checkout a new branch via `kar::workbench_git::Repository`.
pub fn switch_or_create_git_branch(workspace: &Path, branch: &str, create: bool) -> Result<String> {
    let clean = branch.trim();
    if clean.is_empty() {
        anyhow::bail!("分支名不能为空");
    }
    let repo = Repository::open(workspace)?;
    if create {
        repo.create_branch(clean, true)?;
        Ok(clean.to_owned())
    } else {
        let checked = repo.switch_branch(clean)?;
        Ok(checked.unwrap_or_else(|| clean.to_owned()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceTreeItem {
    pub id: String,
    pub name: String,
    pub rel_path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<WorkspaceTreeItem>,
}

/// Scan workspace directory into a bounded, clean hierarchy for KUI `TreeView` (dot-folders hidden by default).
pub fn scan_workspace_tree(workspace: &Path, max_nodes: usize) -> Vec<WorkspaceTreeItem> {
    scan_workspace_tree_with_options(workspace, max_nodes, false)
}

/// Scan workspace directory with explicit control over hidden dot-files/folders.
pub fn scan_workspace_tree_with_options(
    workspace: &Path,
    max_nodes: usize,
    show_hidden: bool,
) -> Vec<WorkspaceTreeItem> {
    let mut budget = max_nodes.max(16);
    scan_dir_children(workspace, workspace, 0, &mut budget, show_hidden)
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | "build"
            | "node_modules"
            | ".next"
            | "dist"
            | ".kar-desktop-chats"
            | ".kar-desktop-docs"
            | ".kar-worktrees"
            | ".DS_Store"
    )
}

fn scan_dir_children(
    root: &Path,
    dir: &Path,
    depth: usize,
    budget: &mut usize,
    show_hidden: bool,
) -> Vec<WorkspaceTreeItem> {
    if depth > 3 || *budget == 0 {
        return Vec::new();
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = read_dir.filter_map(std::result::Result::ok).collect();
    entries.sort_by(|a, b| {
        let a_dir = a.file_type().is_ok_and(|t| t.is_dir());
        let b_dir = b.file_type().is_ok_and(|t| t.is_dir());
        let a_name = a.file_name().to_string_lossy().into_owned();
        let b_name = b.file_name().to_string_lossy().into_owned();
        let a_dot = a_name.starts_with('.');
        let b_dot = b_name.starts_with('.');
        // Non-dot directories first, then dot directories (if shown), then files alphabetically
        b_dir
            .cmp(&a_dir)
            .then_with(|| a_dot.cmp(&b_dot))
            .then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
    });

    let mut out = Vec::new();
    for entry in entries {
        if *budget == 0 {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if should_skip_dir(&name) || (!show_hidden && name.starts_with('.')) {
            continue;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let id = rel.to_string_lossy().into_owned();
        *budget = budget.saturating_sub(1);
        if ft.is_dir() {
            let children = scan_dir_children(root, &path, depth + 1, budget, show_hidden);
            out.push(WorkspaceTreeItem {
                id,
                name,
                rel_path: rel,
                is_dir: true,
                children,
            });
        } else if ft.is_file() {
            out.push(WorkspaceTreeItem {
                id,
                name,
                rel_path: rel,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Workspace Skills, Slash Commands & MCP Inventory (`kar::inventory`)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InventoryTab {
    #[default]
    All,
    Skills,
    McpServers,
    Commands,
    Tools,
}

impl InventoryTab {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Skills,
        Self::McpServers,
        Self::Commands,
        Self::Tools,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Skills => "技能",
            Self::McpServers => "MCP 服务",
            Self::Commands => "指令",
            Self::Tools => "工具",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCardItem {
    pub name: String,
    pub description: String,
    pub scope_label: String,
    pub path: Option<String>,
    pub preview_markdown: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerCardItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub source: String,
    pub tool_count: usize,
    pub builtin_office: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommandItem {
    pub name: String,
    pub description: String,
    pub hint: Option<String>,
    pub source: String,
}

impl SlashCommandItem {
    pub(crate) fn normalized_name(raw: &str) -> Option<String> {
        let raw = raw.trim();
        let name = raw.strip_prefix('/').unwrap_or(raw);
        if name.is_empty() || name.starts_with('/') || name.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
            return None;
        }
        Some(format!("/{name}"))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSchemaItem {
    pub name: String,
    pub description: String,
    pub server_id: String,
    pub schema_json: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInventorySnapshot {
    pub runtime_id: String,
    #[serde(default)]
    workspace: PathBuf,
    pub observed_at: String,
    pub skills: Vec<SkillCardItem>,
    pub mcp_servers: Vec<McpServerCardItem>,
    pub commands: Vec<SlashCommandItem>,
    #[serde(default)]
    command_status: CommandCatalogStatus,
    pub tools: Vec<ToolSchemaItem>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
enum CommandCatalogStatus {
    #[default]
    Pending,
    Ready,
    Unavailable,
}

impl WorkspaceInventorySnapshot {
    pub(crate) fn belongs_to(&self, workspace: &Path, runtime_id: &str) -> bool {
        self.workspace == workspace && self.runtime_id == runtime_id
    }

    /// Kar currently exposes workspace command discovery for Claude and Pi.
    /// A cold or failed lookup on those runtimes cannot disprove a native command.
    pub(crate) fn commands_for(
        &self,
        workspace: &Path,
        runtime_id: &str,
    ) -> Option<&[SlashCommandItem]> {
        if self.belongs_to(workspace, runtime_id)
            && self.command_status == CommandCatalogStatus::Ready
        {
            Some(&self.commands)
        } else if matches!(runtime_id, "claude" | "pi") {
            None
        } else {
            Some(&[])
        }
    }

    /// A partial probe still proves that the commands it returned are available.
    pub(crate) fn advertised_commands_for(&self, workspace: &Path, runtime_id: &str) -> &[SlashCommandItem] {
        if self.belongs_to(workspace, runtime_id) {
            &self.commands
        } else {
            &[]
        }
    }

    fn record_command_result(&mut self, result: kar::Result<InventoryResult<CommandEntry>>) {
        self.commands.clear();
        let Ok(InventoryResult::Ok { items, partial, .. }) = result else {
            self.command_status = CommandCatalogStatus::Unavailable;
            return;
        };
        self.command_status = if partial {
            CommandCatalogStatus::Unavailable
        } else {
            CommandCatalogStatus::Ready
        };
        for cmd in items {
            let description = cmd.description.unwrap_or_default();
            let source = cmd.source.unwrap_or_else(|| runtime_display_name(&self.runtime_id));
            for name in std::iter::once(cmd.name).chain(cmd.aliases.into_iter().flatten()) {
                if let Some(name) = SlashCommandItem::normalized_name(&name)
                    && !self.commands.iter().any(|item| item.name == name)
                {
                    self.commands.push(SlashCommandItem {
                        name,
                        description: description.clone(),
                        hint: cmd.argument_hint.clone(),
                        source: source.clone(),
                    });
                }
            }
        }
    }
}

/// Synchronously seed local skills; native capabilities arrive from the runtime probe.
pub fn seed_local_workspace_inventory(
    workspace: &Path,
    runtime_id: &str,
) -> WorkspaceInventorySnapshot {
    WorkspaceInventorySnapshot {
        workspace: workspace.to_owned(),
        runtime_id: runtime_id.to_owned(),
        skills: scan_local_skill_markdown_files(workspace),
        ..Default::default()
    }
}

/// Probe real workspace Skills, MCP servers, Slash commands, and Tools via `kar::inventory`,
/// enriched with local `SKILL.md` discovery and the built-in `kar_office` MCP toolset.
pub async fn probe_workspace_inventory(
    workspace: &Path,
    runtime_id: &str,
) -> WorkspaceInventorySnapshot {
    probe_inventory_with_office(workspace, runtime_id, false).await
}

pub async fn probe_inventory_with_office(workspace: &Path, runtime_id: &str, office_available: bool) -> WorkspaceInventorySnapshot {
    let mut snap = seed_local_workspace_inventory(workspace, runtime_id);
    if office_available {
        use crate::mcp_office::{OFFICE_SERVER_NAME, office_tool_specs};
        snap.tools = office_tool_specs().into_iter().map(|(name, description, schema_json)| ToolSchemaItem {
            name: name.into(), description: description.into(), schema_json, server_id: OFFICE_SERVER_NAME.into(),
        }).collect();
        snap.mcp_servers.push(McpServerCardItem {
            id: OFFICE_SERVER_NAME.into(), name: "Kantor Office".into(), status: "可连接".into(),
            source: "Kwrok 桌面端".into(), tool_count: snap.tools.len(), builtin_office: true, error: None,
        });
    }
    snap.observed_at = "刚刚同步".into();

    // Query `kar::runtime(runtime_id)` inventory concurrently with bounded timeout so UI never stalls
    if let Ok(rt) = kar::runtime(runtime_id) {
        let mut opts = InventoryOptions::default();
        opts.installation.cwd = Some(workspace.to_path_buf());
        opts.installation
            .env
            .insert("PATH".into(), Some(crate::kar_bridge::augmented_path_os()));
        opts.timeout = Some(Duration::from_millis(1100));
        let command_opts = InventoryOptions {
            // A cold CLI must finish its initialization handshake to advertise commands.
            timeout: None,
            ..opts.clone()
        };

        let (skills_res, mcp_res, cmds_res, tools_res) = tokio::join!(
            rt.skills(&opts),
            rt.mcp_servers(&opts),
            rt.commands(&command_opts),
            rt.tools(&opts),
        );

        if let Ok(InventoryResult::Ok { items, .. }) = skills_res {
            for sk in items {
                if snap.skills.iter().any(|s| s.name == sk.name) {
                    continue;
                }
                let preview = sk
                    .path
                    .as_deref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .map(|s| truncate_chars(s.as_str(), 1800));
                let scope_label = match sk.scope {
                    Some(SkillScope::Project) => "项目技能",
                    Some(SkillScope::User) => "用户全局",
                    Some(SkillScope::Builtin) => "内置技能",
                    Some(SkillScope::Managed) => "托管插件",
                    _ => "已发现",
                }
                .to_owned();
                snap.skills.push(SkillCardItem {
                    name: sk.name,
                    description: sk
                        .description
                        .unwrap_or_else(|| "工作区可调用专业技能包".into()),
                    scope_label,
                    path: sk.path,
                    preview_markdown: preview,
                    enabled: sk.enabled.unwrap_or(true),
                });
            }
        }

        if let Ok(InventoryResult::Ok { items, .. }) = mcp_res {
            for srv in items {
                if snap.mcp_servers.iter().any(|m| m.id == srv.id) {
                    continue;
                }
                snap.mcp_servers.push(McpServerCardItem {
                    id: srv.id,
                    name: srv.name,
                    status: srv.status.unwrap_or_else(|| "已配置".into()),
                    source: srv
                        .source
                        .unwrap_or_else(|| runtime_display_name(runtime_id)),
                    tool_count: srv.tool_count.unwrap_or(0),
                    builtin_office: false,
                    error: srv.error,
                });
            }
        }

        snap.record_command_result(cmds_res);

        if let Ok(InventoryResult::Ok { items, .. }) = tools_res {
            for t in items {
                let server_id = t.mcp_server_id.or(t.source).unwrap_or_else(|| runtime_display_name(runtime_id));
                if snap.tools.iter().any(|existing| existing.name == t.name && existing.server_id == server_id) {
                    continue;
                }
                snap.tools.push(ToolSchemaItem {
                    name: t.name,
                    description: t.description.unwrap_or_else(|| "智能体可调用工具".into()),
                    server_id,
                    schema_json: t
                        .input_schema
                        .map(Value::Object)
                        .unwrap_or(Value::Null),
                });
            }
        }
    }

    snap
}

fn scan_local_skill_markdown_files(workspace: &Path) -> Vec<SkillCardItem> {
    let mut found = Vec::new();
    let candidates = [
        workspace.join(".agents/skills"),
        workspace.join("skills"),
        workspace.join(".claude/skills"),
    ];
    for dir in candidates {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.filter_map(std::result::Result::ok) {
            let skill_md = entry.path().join("SKILL.md");
            if let Ok(raw) = std::fs::read_to_string(&skill_md) {
                let fallback_name = entry.file_name().to_string_lossy().into_owned();
                let (name, desc) = parse_skill_frontmatter(&raw, &fallback_name);
                found.push(SkillCardItem {
                    name,
                    description: desc,
                    scope_label: "项目技能".into(),
                    path: Some(skill_md.display().to_string()),
                    preview_markdown: Some(truncate_chars(&raw, 1800)),
                    enabled: true,
                });
            }
        }
    }
    found
}

fn parse_skill_frontmatter(raw: &str, fallback_name: &str) -> (String, String) {
    let mut name = fallback_name.to_owned();
    let mut desc = String::new();
    if let Some(after_open) = raw.strip_prefix("---")
        && let Some((fm, _)) = after_open.split_once("\n---")
    {
        for line in fm.lines() {
            if let Some(v) = line.strip_prefix("name:") {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    name = v.to_owned();
                }
            } else if let Some(v) = line.strip_prefix("description:") {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    desc = v.to_owned();
                }
            }
        }
    }
    if desc.is_empty() {
        desc = raw
            .lines()
            .find(|l| !l.trim().is_empty() && !l.starts_with('#') && !l.starts_with("---"))
            .unwrap_or("本地 SKILL.md 专业技能指导")
            .trim()
            .to_owned();
    }
    (name, desc)
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Global Multi-Kind Search (Workspace Paths, File Lines & Chat Messages)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalSearchHit {
    FilePath {
        rel_path: PathBuf,
    },
    FileContent {
        rel_path: PathBuf,
        line: usize,
        snippet: String,
    },
    ChatMessage {
        chat_id: Uuid,
        chat_title: String,
        role_label: &'static str,
        snippet: String,
    },
}

pub fn run_global_search(
    workspace: &Path,
    store: Option<&ChatStore>,
    query: &str,
    limit_per_kind: usize,
) -> Vec<GlobalSearchHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let cap = limit_per_kind.clamp(3, 40);
    let mut hits = Vec::new();

    // 1. Search saved chat messages in `ChatStore`
    if let Some(chat_store) = store
        && let Ok(infos) = chat_store.list()
    {
        let mut msg_count = 0usize;
        for info in infos {
            if msg_count >= cap {
                break;
            }
            if let Ok(h) = chat_store.history(info.id) {
                let chat_title = format!("{} 对话", runtime_display_name(&info.runtime));
                for turn in h.turns {
                    for entry in turn.items {
                        if let Item::Message { role, text, .. } = entry.item
                            && text.to_lowercase().contains(&needle)
                        {
                            let role_label = match role {
                                Role::User => "提问",
                                Role::Assistant => "回复",
                                _ => "消息",
                            };
                            hits.push(GlobalSearchHit::ChatMessage {
                                chat_id: info.id,
                                chat_title: chat_title.clone(),
                                role_label,
                                snippet: extract_match_line(&text, &needle),
                            });
                            msg_count += 1;
                            if msg_count >= cap {
                                break;
                            }
                        }
                    }
                    if msg_count >= cap {
                        break;
                    }
                }
            }
        }
    }

    // 2. Search workspace file paths & line contents
    let mut files_walked = 0usize;
    let mut path_hits = 0usize;
    let mut line_hits = 0usize;
    walk_workspace_files_for_search(
        workspace,
        workspace,
        0,
        &needle,
        cap,
        &mut files_walked,
        &mut path_hits,
        &mut line_hits,
        &mut hits,
    );

    hits
}

#[allow(clippy::too_many_arguments)]
fn walk_workspace_files_for_search(
    root: &Path,
    dir: &Path,
    depth: usize,
    needle: &str,
    cap: usize,
    files_walked: &mut usize,
    path_hits: &mut usize,
    line_hits: &mut usize,
    hits: &mut Vec<GlobalSearchHit>,
) {
    if depth > 4 || *files_walked > 600 || (*path_hits >= cap && *line_hits >= cap) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.filter_map(std::result::Result::ok) {
        if *files_walked > 600 || (*path_hits >= cap && *line_hits >= cap) {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if should_skip_dir(&name) || name.starts_with('.') {
            continue;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if ft.is_dir() {
            walk_workspace_files_for_search(
                root,
                &path,
                depth + 1,
                needle,
                cap,
                files_walked,
                path_hits,
                line_hits,
                hits,
            );
        } else if ft.is_file() {
            *files_walked += 1;
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let rel_str = rel.to_string_lossy();
            if *path_hits < cap && rel_str.to_lowercase().contains(needle) {
                hits.push(GlobalSearchHit::FilePath {
                    rel_path: rel.clone(),
                });
                *path_hits += 1;
            }
            if *line_hits < cap
                && let Ok(meta) = entry.metadata()
                && meta.len() <= 256 * 1024
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                for (idx, line) in text.lines().enumerate() {
                    if line.to_lowercase().contains(needle) {
                        hits.push(GlobalSearchHit::FileContent {
                            rel_path: rel.clone(),
                            line: idx + 1,
                            snippet: truncate_chars(line.trim(), 96),
                        });
                        *line_hits += 1;
                        break;
                    }
                }
            }
        }
    }
}

fn extract_match_line(text: &str, needle: &str) -> String {
    let line = text
        .lines()
        .find(|l| l.to_lowercase().contains(needle))
        .or_else(|| text.lines().next())
        .unwrap_or("")
        .trim();
    truncate_chars(line, 96)
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Usage, Prompt Cache & Quota Analytics (`UsageAnalyticsSnapshot`)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeTokenRow {
    pub runtime_id: String,
    pub display_name: String,
    pub chat_count: usize,
    pub turn_count: usize,
    pub has_usage: bool,
    pub has_cache_read: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub reasoning_tokens: u64,
    pub quota_summary: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageAnalyticsSnapshot {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_cache_write_tokens: u64,
    pub total_reasoning_tokens: u64,
    pub total_chats: usize,
    pub total_turns: usize,
    pub cache_hit_ratio: f32,
    pub has_usage: bool,
    pub has_cache_read: bool,
    pub has_cache_write: bool,
    pub has_reasoning: bool,
    pub by_runtime: Vec<RuntimeTokenRow>,
    /// Each inherited turn is credited to a single retained chat.
    pub turns_by_chat: BTreeMap<Uuid, usize>,
    pub turns_by_day: BTreeMap<chrono::NaiveDate, usize>,
    /// The last 28 local calendar dates, oldest first and including today.
    pub daily_activity: Vec<(String, f64)>,
}

/// Forks persist entire turn prefixes. Scope the caller's idempotency key to its
/// lineage and original execution so unrelated chats or sibling calls stay distinct.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct UsageTurnId {
    lineage: Uuid,
    input: Uuid,
    started_ms: u64,
    native_session: Option<String>,
}

#[derive(Clone)]
struct UsageTurnOwner {
    chat: Uuid,
    runtime: Option<String>,
    inherited: bool,
}

impl UsageTurnOwner {
    fn priority(&self) -> (bool, bool, Uuid) {
        // The original wins independently of journal order and clock skew.
        // Retain one deterministic attribution if only archived copies survive.
        (self.inherited, self.runtime.is_none(), self.chat)
    }
}

fn record_usage_turns(
    history: &kar::chat::ChatHistory,
    turns: &mut BTreeMap<UsageTurnId, UsageTurnOwner>,
) {
    let info = &history.chat;
    let fork = info.forked_from.as_ref();
    let lineage = fork.map_or(info.id, |origin| origin.checkpoint_store);
    let cut = fork.and_then(|origin| history.turns.iter().position(|t| t.id == origin.turn));
    let handoffs = history
        .handoffs
        .iter()
        .filter(|h| h.status == kar::chat::HandoffStatus::Ready);
    let initial_runtime = handoffs
        .clone()
        .next()
        .map_or(&info.runtime, |h| &h.from.runtime);
    for (index, turn) in history.turns.iter().enumerate() {
        let route = handoffs.clone().find(|h| index < h.through_turn);
        let (runtime, native_id) = route.map_or((&info.runtime, &info.native_id), |h| {
            (&h.from.runtime, &h.from.native_id)
        });
        let source = turn
            .items
            .iter()
            .find(|item| item.source.path.is_empty())
            .and_then(|item| item.source.session_id.as_deref());
        let witnessed = source.is_some_and(|s| s == native_id);
        // A rewind can remove the fork cut, leaving only its native session witness.
        let inherited = fork.is_some() && cut.map_or(!witnessed, |end| index <= end);
        let runtime = if !inherited || witnessed {
            Some(runtime.clone())
        } else if fork.is_some_and(|origin| source == Some(origin.native_source.as_str())) {
            Some(initial_runtime.clone())
        } else {
            None
        };
        let key = UsageTurnId {
            lineage,
            input: turn.id,
            started_ms: turn.started_ms,
            native_session: source.map(str::to_owned),
        };
        let owner = UsageTurnOwner {
            chat: info.id,
            inherited,
            runtime,
        };
        turns
            .entry(key)
            .and_modify(|current| {
                if owner.priority() < current.priority() {
                    *current = owner.clone();
                }
            })
            .or_insert(owner);
    }
}

pub fn compute_usage_analytics(
    store: Option<&ChatStore>,
    active_usage: &[UsageSnapshot],
    providers: &[ProviderStatusInfo],
) -> UsageAnalyticsSnapshot {
    let today = chrono::Local::now().date_naive();
    let mut turn_owners = BTreeMap::new();
    let mut has_usage = false;
    let mut has_cache_read = false;
    let mut has_cache_write = false;
    let mut has_reasoning = false;
    let mut observe_fields = |tot: &kar::TokenTotals| {
        has_usage = true;
        has_cache_read |= tot.cache_read.is_some();
        has_cache_write |= tot.cache_write.is_some();
        has_reasoning |= tot.reasoning.is_some();
    };
    let mut rows: BTreeMap<String, RuntimeTokenRow> = BTreeMap::new();
    let mut total_in = 0u64;
    let mut total_out = 0u64;
    let mut total_cached = 0u64;
    let mut total_cache_write = 0u64;
    let mut total_reasoning = 0u64;
    let mut total_chats = 0usize;

    if let Some(chat_store) = store
        && let Ok(infos) = chat_store.list()
    {
        total_chats = infos.len();
        for info in infos {
            let entry = rows
                .entry(info.runtime.clone())
                .or_insert_with(|| RuntimeTokenRow {
                    runtime_id: info.runtime.clone(),
                    display_name: runtime_display_name(&info.runtime),
                    ..Default::default()
                });
            entry.chat_count += 1;
            if let Ok(h) = chat_store.history(info.id) {
                record_usage_turns(&h, &mut turn_owners);
                for u in &h.usage {
                    if let Some(tot) = u.usage.with_children.or(u.usage.total) {
                        observe_fields(&tot);
                        entry.has_usage = true;
                        entry.has_cache_read |= tot.cache_read.is_some();
                        let inp = tot.input.max(0) as u64;
                        let out = tot.output.max(0) as u64;
                        let cr = tot.cache_read.unwrap_or(0).max(0) as u64;
                        let cw = tot.cache_write.unwrap_or(0).max(0) as u64;
                        let rs = tot.reasoning.unwrap_or(0).max(0) as u64;
                        entry.input_tokens = entry.input_tokens.saturating_add(inp);
                        entry.output_tokens = entry.output_tokens.saturating_add(out);
                        entry.cached_tokens = entry.cached_tokens.saturating_add(cr);
                        entry.reasoning_tokens = entry.reasoning_tokens.saturating_add(rs);
                        total_in = total_in.saturating_add(inp);
                        total_out = total_out.saturating_add(out);
                        total_cached = total_cached.saturating_add(cr);
                        total_cache_write = total_cache_write.saturating_add(cw);
                        total_reasoning = total_reasoning.saturating_add(rs);
                    }
                }
            }
        }
    }

    let total_turns = turn_owners.len();
    let mut turns_by_chat = BTreeMap::new();
    let mut turns_by_day = BTreeMap::new();
    for (turn, owner) in turn_owners {
        *turns_by_chat.entry(owner.chat).or_default() += 1;
        if let Some(runtime_id) = owner.runtime {
            rows.entry(runtime_id.clone())
                .or_insert_with(|| RuntimeTokenRow {
                    display_name: runtime_display_name(&runtime_id),
                    runtime_id,
                    ..Default::default()
                })
                .turn_count += 1;
        }
        if let Ok(ms) = i64::try_from(turn.started_ms)
            && let Some(at) = chrono::DateTime::from_timestamp_millis(ms)
        {
            let day = at.with_timezone(&chrono::Local).date_naive();
            if day <= today {
                *turns_by_day.entry(day).or_default() += 1;
            }
        }
    }
    let first_day = today - chrono::Duration::days(27);
    let daily_activity = (0..28)
        .map(|offset| {
            let day = first_day + chrono::Duration::days(offset);
            (
                day.to_string(),
                turns_by_day.get(&day).copied().unwrap_or(0) as f64,
            )
        })
        .collect();

    // Fold active live usage if no saved history yet (e.g. Demo mode)
    if !rows.values().any(|row| row.has_usage) {
        for u in active_usage {
            if let Some(tot) = u.usage.with_children.or(u.usage.total) {
                observe_fields(&tot);
                total_in = total_in.saturating_add(tot.input.max(0) as u64);
                total_out = total_out.saturating_add(tot.output.max(0) as u64);
                total_cached =
                    total_cached.saturating_add(tot.cache_read.unwrap_or(0).max(0) as u64);
                total_cache_write =
                    total_cache_write.saturating_add(tot.cache_write.unwrap_or(0).max(0) as u64);
                total_reasoning =
                    total_reasoning.saturating_add(tot.reasoning.unwrap_or(0).max(0) as u64);
            }
        }
    }

    // Attach live provider quota summaries
    for prov in providers {
        if let Some(ref q) = prov.quota_summary {
            let row = rows
                .entry(prov.id.clone())
                .or_insert_with(|| RuntimeTokenRow {
                    runtime_id: prov.id.clone(),
                    display_name: prov.display_name(),
                    ..Default::default()
                });
            row.quota_summary = Some(q.clone());
        }
    }

    let mut by_runtime: Vec<RuntimeTokenRow> = rows.into_values().collect();
    by_runtime.sort_by(|a, b| {
        b.input_tokens
            .saturating_add(b.output_tokens)
            .cmp(&a.input_tokens.saturating_add(a.output_tokens))
    });

    let cache_hit_ratio = if total_in > 0 {
        ((total_cached as f64 / total_in as f64) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    };

    UsageAnalyticsSnapshot {
        total_input_tokens: total_in,
        total_output_tokens: total_out,
        total_cached_tokens: total_cached,
        total_cache_write_tokens: total_cache_write,
        total_reasoning_tokens: total_reasoning,
        total_chats,
        total_turns,
        cache_hit_ratio,
        by_runtime,
        has_usage,
        has_cache_read,
        has_cache_write,
        has_reasoning,
        daily_activity,
        turns_by_chat,
        turns_by_day,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Portable Cross-Runtime Session Handoff Context Pack
// ═══════════════════════════════════════════════════════════════════════════

/// Build a bounded portable context summary (`<= budget_bytes`) when handing off a chat
/// from one runtime (e.g. `Codex`) to another (e.g. `Claude Code`, `Cursor`, `Antigravity`).
pub fn build_portable_handoff_context(
    source_runtime: &str,
    target_runtime: &str,
    turns_summary: &[(String, String)],
    user_note: &str,
    budget_bytes: usize,
) -> String {
    let cap = budget_bytes.clamp(1024, 128 * 1024);
    let mut header = format!(
        "[跨智能体会话接力摘要 · 由 {} 移交至 {}]\n以下为前序会话关键上下文，请直接基于既有结论继续推进：\n\n",
        runtime_display_name(source_runtime),
        runtime_display_name(target_runtime),
    );
    let mut footer = if user_note.trim().is_empty() {
        "\n请确认已接手上述上下文，并给出下一步执行建议。".to_owned()
    } else {
        format!("\n[接力后新指令]\n{}", user_note.trim())
    };

    let remaining = cap.saturating_sub(header.len());
    if footer.len() > remaining {
        let mut end = remaining.saturating_sub('…'.len_utf8());
        while !footer.is_char_boundary(end) {
            end -= 1;
        }
        footer.truncate(end);
        footer.push('…');
    }
    let reserve = header.len() + footer.len();
    let body_budget = cap.saturating_sub(reserve);
    let mut chunks = Vec::new();
    let mut used = 0usize;
    for (idx, (prompt, reply)) in turns_summary.iter().enumerate().rev() {
        let block = format!(
            "- 回合 #{}: 用户「{}」 -> 结论「{}」\n",
            idx + 1,
            truncate_chars(prompt.trim(), 240),
            truncate_chars(reply.trim(), 380),
        );
        if used + block.len() > body_budget {
            break;
        }
        used += block.len();
        chunks.push(block);
    }
    chunks.reverse();
    for ch in chunks {
        header.push_str(&ch);
    }
    header.push_str(&footer);
    header
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Structured Diagnostics Ring Buffer (`LogStream` Adapter)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagLogRecord {
    pub id: String,
    pub timestamp: String,
    pub level: DiagLevel,
    pub source: String,
    pub tag: Option<String>,
    pub message: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct DiagLogRing {
    max_entries: usize,
    seq: u64,
    entries: VecDeque<DiagLogRecord>,
}

impl Default for DiagLogRing {
    fn default() -> Self {
        let mut ring = Self::new(300);
        ring.push(
            DiagLevel::Info,
            "workbench",
            Some("BOOT"),
            "Kwrok GPUI + KUI 渲染管线与 Tokio 桥接线程初始化完成",
            vec![("unsafe_code".into(), "forbid".into()), ("renderer".into(), "Metal GPUI".into())],
        );
        ring
    }
}

impl DiagLogRing {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(16),
            seq: 0,
            entries: VecDeque::new(),
        }
    }

    pub fn push(
        &mut self,
        level: DiagLevel,
        source: impl Into<String>,
        tag: Option<&str>,
        message: impl Into<String>,
        fields: Vec<(String, String)>,
    ) {
        self.seq = self.seq.saturating_add(1);
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(DiagLogRecord {
            id: format!("log-{}", self.seq),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            source: source.into(),
            tag: tag.map(str::to_owned),
            message: message.into(),
            fields,
        });
    }

    pub(crate) fn revision(&self) -> u64 {
        self.seq
    }

    pub fn records(&self) -> Vec<DiagLogRecord> {
        self.entries.iter().cloned().collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Projects, Pinned Threads & Managed Git Worktrees (`ProjectRegistry`)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudioProject {
    pub id: String,
    pub name: String,
    pub root_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatWorktreeMeta {
    pub worktree_id: Uuid,
    pub repo_root: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRegistry {
    pub projects: Vec<StudioProject>,
    pub pinned_chats: BTreeSet<Uuid>,
    pub collapsed_projects: BTreeSet<String>,
    pub chat_worktrees: BTreeMap<Uuid, ChatWorktreeMeta>,
}

fn project_registry_file(chat_dir: &Path) -> PathBuf {
    chat_dir.join("kar_desktop_projects.json")
}

fn normalize_project_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A display alias for the generic workspace folder. Storage and routing use its original path.
pub(crate) fn project_display_name(name: &str) -> &str {
    let name = name.trim();
    if name.is_empty()
        || name.eq_ignore_ascii_case("workspace")
        || name.eq_ignore_ascii_case("work space")
    {
        "Kwork"
    } else {
        name
    }
}

fn derive_project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

impl ProjectRegistry {
    pub fn load_or_seed(chat_dir: &Path, initial_workspace: &Path) -> Self {
        let file = project_registry_file(chat_dir);
        let mut reg: Self = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        reg.ensure_project(initial_workspace);
        reg
    }

    pub fn save(&self, chat_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(chat_dir)?;
        let bytes = serde_json::to_vec_pretty(self)?;
        std::fs::write(project_registry_file(chat_dir), bytes)?;
        Ok(())
    }

    pub fn ensure_project(&mut self, dir: &Path) -> StudioProject {
        let norm = normalize_project_path(dir);
        if let Some(existing) = self
            .projects
            .iter()
            .find(|p| normalize_project_path(&p.root_path) == norm)
        {
            return existing.clone();
        }
        let name = derive_project_name(&norm);
        let id = format!(
            "proj-{}-{}",
            name.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>()
                .to_ascii_lowercase(),
            self.projects.len() + 1
        );
        let proj = StudioProject {
            id,
            name,
            root_path: norm,
        };
        self.projects.push(proj.clone());
        proj
    }

    pub fn is_pinned(&self, chat_id: Uuid) -> bool {
        self.pinned_chats.contains(&chat_id)
    }

    pub fn toggle_pin(&mut self, chat_id: Uuid) -> bool {
        if !self.pinned_chats.remove(&chat_id) {
            self.pinned_chats.insert(chat_id);
            true
        } else {
            false
        }
    }

    pub fn is_collapsed(&self, project_id: &str) -> bool {
        self.collapsed_projects.contains(project_id)
    }

    pub fn toggle_collapsed(&mut self, project_id: &str) {
        if !self.collapsed_projects.remove(project_id) {
            self.collapsed_projects.insert(project_id.to_owned());
        }
    }

    pub fn worktree_for(&self, chat_id: Uuid) -> Option<&ChatWorktreeMeta> {
        self.chat_worktrees.get(&chat_id)
    }

    /// Match a chat workspace (or its parent repo root when running inside an isolated worktree)
    /// to a registered `StudioProject`.
    pub fn project_for_chat(&self, chat_workspace: &Path, chat_id: Uuid) -> Option<&StudioProject> {
        let target = self
            .chat_worktrees
            .get(&chat_id)
            .map(|wt| normalize_project_path(&wt.repo_root))
            .unwrap_or_else(|| normalize_project_path(chat_workspace));
        self.projects
            .iter()
            .filter(|p| {
                let root = normalize_project_path(&p.root_path);
                target == root || target.starts_with(&root)
            })
            .max_by_key(|p| p.root_path.as_os_str().len())
    }
}

/// Create a genuine Kar-managed isolated Git worktree (`kar::workbench_git::Repository::create_worktree`)
/// outside existing repository checkouts so an agent session can edit, build, and test in total isolation.
pub fn create_isolated_worktree_for_chat(
    workspace: &Path,
    store_dir: &Path,
    worktree_id: Uuid,
) -> Result<ChatWorktreeMeta> {
    let repo = Repository::open(workspace)?;
    let repo_root = repo.root().to_path_buf();
    let canon_repo = normalize_project_path(&repo_root);
    let canon_store = normalize_project_path(store_dir);

    let repo_slug: String = derive_project_name(&canon_repo)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = if repo_slug.trim_matches('-').is_empty() {
        "repo"
    } else {
        repo_slug.trim_matches('-')
    };
    let short_id = &worktree_id.simple().to_string()[..8];

    // `Repository::create_worktree` requires a new directory strictly outside existing checkouts.
    let base_dir = if !canon_store.starts_with(&canon_repo) && !canon_repo.starts_with(&canon_store)
    {
        store_dir.join("worktrees")
    } else {
        std::env::temp_dir().join("kar-desktop-worktrees")
    };
    std::fs::create_dir_all(&base_dir)?;
    let dest = base_dir.join(format!("{slug}-{short_id}"));
    let branch = format!("kar/wt-{short_id}");

    let managed = repo.create_worktree(worktree_id, &dest, &branch, "HEAD")?;
    Ok(ChatWorktreeMeta {
        worktree_id: managed.id,
        repo_root: canon_repo,
        worktree_path: managed.path,
        branch: managed.branch,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Integrated Workspace Terminal Runner Models (`ui_terminal.rs`)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRunRecord {
    pub id: Uuid,
    pub command: String,
    pub cwd: PathBuf,
    pub output: String,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub started_at: String,
}

pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_inventory_is_only_authoritative_for_its_runtime_and_workspace() {
        use kar::inventory::{CommandEntry, InventoryScope, View};
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path();
        let mut snap = seed_local_workspace_inventory(workspace, "claude");
        assert_eq!(snap.command_status, CommandCatalogStatus::Pending);
        assert!(snap.commands_for(workspace, "claude").is_none());
        assert_eq!(snap.commands_for(workspace, "codex"), Some(&[][..]));

        snap.record_command_result(Ok(InventoryResult::Ok {
            scope: InventoryScope::Workspace { cwd: workspace.to_owned() },
            view: View::Discovered,
            observed_at: String::new(),
            items: vec![CommandEntry {
                name: "plan".into(), description: None, argument_hint: None, aliases: None, source: None,
            }],
            partial: false,
        }));
        assert_eq!(snap.commands_for(workspace, "claude").unwrap()[0].name, "/plan");
        assert!(snap.commands_for(&workspace.join("other"), "claude").is_none());
        assert!(snap.commands_for(workspace, "pi").is_none());
    }

    #[test]
    fn command_inventory_timeout_and_partial_results_keep_native_routing_unknown() {
        use kar::inventory::{InventoryScope, UnavailableCode, View};
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path();
        let mut snap = seed_local_workspace_inventory(workspace, "pi");
        snap.record_command_result(Ok(InventoryResult::Unavailable {
            code: UnavailableCode::Timeout, reason: "cold start".into(),
        }));
        assert_eq!(snap.command_status, CommandCatalogStatus::Unavailable);
        assert!(snap.commands_for(workspace, "pi").is_none());
        for partial in [true, false] {
            snap.record_command_result(Ok(InventoryResult::Ok {
                scope: InventoryScope::Workspace { cwd: workspace.to_owned() },
                view: View::Discovered,
                observed_at: String::new(),
                items: Vec::new(),
                partial,
            }));
            assert_eq!(snap.commands_for(workspace, "pi"), (!partial).then_some(&[][..]));
        }
    }

    #[test]
    fn partial_inventory_keeps_reported_names_and_aliases_scoped_to_the_workspace() {
        use kar::inventory::{CommandEntry, InventoryScope, View};
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path();
        let mut snap = seed_local_workspace_inventory(workspace, "claude");
        snap.record_command_result(Ok(InventoryResult::Ok {
            scope: InventoryScope::Workspace { cwd: workspace.to_owned() },
            view: View::Discovered,
            observed_at: String::new(),
            items: vec![CommandEntry {
                name: "/explain".into(),
                aliases: Some(vec!["exp".into(), " /explain ".into()]),
                description: None, source: None, argument_hint: None,
            }],
            partial: true,
        }));
        let names: Vec<_> = snap.advertised_commands_for(workspace, "claude")
            .iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, ["/explain", "/exp"]);
        assert!(snap.commands_for(workspace, "claude").is_none());
        assert!(snap.advertised_commands_for(workspace, "codex").is_empty());
        assert!(snap.advertised_commands_for(&workspace.join("other"), "claude").is_empty());
    }

    fn write_usage_journal(
        directory: &Path,
        id: Uuid,
        runtime: &str,
        fork: Option<Value>,
        created_ms: i64,
        updates: Vec<Value>,
    ) {
        use serde_json::json;
        let header = json!({"format": "kar-chat/1", "chat": {
            "id": id, "workspace": directory, "runtime": runtime, "protocol": "codex",
            "native_id": id.to_string(), "program": "codex", "created_ms": created_ms,
            "approvals": "host", "forked_from": fork
        }});
        let journal = std::iter::once(header)
            .chain(updates)
            .map(|row| format!("{row}\n"))
            .collect::<String>();
        std::fs::write(directory.join(format!("{id}.karchat")), journal).unwrap();
    }

    #[test]
    fn forked_archives_count_local_activity_once_even_when_the_original_is_missing() {
        use chrono::{Days, Local, TimeZone};
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let store = ChatStore::open(dir.path()).unwrap();
        let today = Local::now().date_naive();
        let first_day = today - Days::new(27);
        let just_after_midnight = |day: chrono::NaiveDate| {
            Local
                .from_local_datetime(&day.and_hms_opt(0, 2, 0).unwrap())
                .earliest()
                .unwrap()
                .timestamp_millis()
        };
        let cut = Uuid::new_v4();
        let ids = [Uuid::from_u128(3), Uuid::from_u128(2), Uuid::from_u128(1)];
        let sample_usage = |stream: Uuid, seq: u64, input: i64| {
            json!({
                "kind":"usage_changed", "reading": {
                    "cursor":{"stream_id":stream, "after_seq":seq},
                    "usage":{"total":{"input":input, "output":1}}
                }
            })
        };
        let mut original = vec![
            json!({"kind":"opened", "stream":ids[0]}),
            sample_usage(ids[0], 1, 12),
            sample_usage(ids[0], 2, 14),
        ];
        for (id, date) in [
            (Uuid::new_v4(), first_day - Days::new(1)),
            (Uuid::new_v4(), first_day),
            (cut, today - Days::new(1)),
        ] {
            original.push(
                json!({"kind":"begin", "id":id, "at":just_after_midnight(date), "text":"Prompt"}),
            );
            original.push(json!({"kind":"end", "id":id, "status":"completed"}));
        }
        write_usage_journal(dir.path(), ids[0], "codex", None, 2000, original);
        let inherited = store.history(ids[0]).unwrap().turns;
        for child in &ids[1..] {
            let fork = json!({"chat":ids[0], "turn":cut, "native_source":ids[0].to_string(),
                "checkpoint_store":ids[0]});
            let turn = Uuid::new_v4();
            write_usage_journal(
                dir.path(),
                *child,
                "codex",
                Some(fork),
                1000,
                vec![
                    json!({"kind":"inherited", "turns":inherited}),
                    json!({"kind":"opened", "stream":child}),
                    sample_usage(*child, 1, 3),
                    json!({"kind":"begin", "id":turn, "at":just_after_midnight(today), "text":"Follow-up"}),
                    json!({"kind":"end", "id":turn, "status":"completed"}),
                ],
            );
        }
        for parent_present in [true, false] {
            if !parent_present {
                std::fs::remove_file(dir.path().join(format!("{}.karchat", ids[0]))).unwrap();
            }
            let usage = compute_usage_analytics(Some(&store), &[], &[]);
            assert_eq!(usage.total_turns, 5);
            assert_eq!(
                usage.total_input_tokens,
                if parent_present { 20 } else { 6 }
            );
            assert_eq!(usage.turns_by_chat.values().sum::<usize>(), 5);
            assert_eq!(usage.turns_by_day.get(&today), Some(&2));
            assert_eq!(usage.by_runtime[0].turn_count, 5);
            assert_eq!(
                usage.daily_activity.first(),
                Some(&(first_day.to_string(), 1.))
            );
            assert_eq!(usage.daily_activity.last(), Some(&(today.to_string(), 2.)));
            assert_eq!(usage.daily_activity.iter().map(|(_, n)| n).sum::<f64>(), 4.);
            if parent_present {
                assert_eq!(usage.turns_by_chat[&ids[0]], 3);
                assert_eq!(usage.turns_by_chat[&ids[2]], 1);
            } else {
                assert_eq!(usage.turns_by_chat[&ids[2]], 4);
            }
        }
    }

    #[test]
    fn runtime_rows_distinguish_missing_usage_from_measured_zero() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let store = ChatStore::open(dir.path()).unwrap();
        for (runtime, totals) in [
            ("codex", None),
            ("claude", Some(json!({"input":0, "output":0}))),
            (
                "gemini",
                Some(json!({"input":0, "output":0, "cache_read":0})),
            ),
        ] {
            let id = Uuid::new_v4();
            let mut updates = vec![json!({"kind":"opened", "stream":id})];
            if let Some(total) = totals {
                updates.push(json!({"kind":"usage_changed", "reading":{
                    "cursor":{"stream_id":id, "after_seq":1}, "usage":{"total":total}
                }}));
            }
            write_usage_journal(dir.path(), id, runtime, None, 1000, updates);
        }
        let active_only = serde_json::from_value(json!({
            "cursor":{"stream_id":Uuid::new_v4(), "after_seq":1},
            "usage":{"total":{"input":900, "output":0}}
        }))
        .unwrap();
        let snap = compute_usage_analytics(Some(&store), &[active_only], &[]);
        assert_eq!(snap.total_input_tokens, 0);
        assert!(snap.has_usage && snap.has_cache_read);
        let row = |id| {
            snap.by_runtime
                .iter()
                .find(|row| row.runtime_id == id)
                .unwrap()
        };
        assert!(!row("codex").has_usage && !row("codex").has_cache_read);
        assert!(row("claude").has_usage && !row("claude").has_cache_read);
        assert!(row("gemini").has_usage && row("gemini").has_cache_read);
        assert_eq!(row("gemini").cached_tokens, 0);
    }

    #[test]
    fn partial_stage_reads_index_and_working_tree_independently() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let git = |args: &[&str]| assert!(Command::new("git").current_dir(root)
            .args(args).output().unwrap().status.success(), "{args:?}");
        git(&["init", "-b", "main"]);
        let path = Path::new("notes.md");
        std::fs::write(root.join(path), "original\n").unwrap();
        git(&["add", "notes.md"]);
        let unborn = read_diff_side(root, path, false, GitDiffSide::Staged);
        assert_eq!(unborn[0].after, ["original"]);
        git(&["-c", "user.name=Test", "-c", "user.email=test@localhost", "commit", "-m", "First"]);
        std::fs::write(root.join(path), "staged\n").unwrap();
        git(&["add", "notes.md"]);
        std::fs::write(root.join(path), "working\n").unwrap();

        let snapshot = discover_git_workspace(root);
        assert_eq!(snapshot.dirty_count(), 1);
        assert!(snapshot.files[0].is_staged() && snapshot.files[0].has_unstaged());
        let staged = read_diff_side(root, path, false, GitDiffSide::Staged);
        let unstaged = read_diff_side(root, path, false, GitDiffSide::WorkingTree);
        assert_eq!(staged[0].before, ["original"]);
        assert_eq!(staged[0].after, ["staged"]);
        assert_eq!(unstaged[0].before, ["staged"]);
        assert_eq!(unstaged[0].after, ["working"]);
    }

    #[test]
    fn unified_diff_parser_extracts_one_based_hunks() {
        let sample = "\
diff --git a/src/lib.rs b/src/lib.rs
index 111..222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -12,2 +12,3 @@ fn demo() {
-    let old = 1;
-    let stale = true;
+    let old = 2;
+    let fresh = true;
+    let verified = true;
";
        let hunks = parse_unified_diff_hunks(sample);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].start_line, 12);
        assert_eq!(hunks[0].before.len(), 2);
        assert_eq!(hunks[0].after.len(), 3);
    }

    #[test]
    fn portable_handoff_context_respects_byte_budget_and_keeps_latest_turns() {
        let turns: Vec<(String, String)> = (1..=30)
            .map(|i| {
                (
                    format!("第 {i} 轮问题：检查模块边界与表格公式"),
                    format!("第 {i} 轮结论：已完成验证并记录修订号 v{i}"),
                )
            })
            .collect();
        let packed = build_portable_handoff_context(
            "codex",
            "claude",
            &turns,
            "请继续完成最终 PR 审查",
            1400,
        );
        assert!(packed.len() <= 1400);
        assert!(packed.contains("Codex"));
        assert!(packed.contains("Claude Code"));
        assert!(packed.contains("第 30 轮"));
        assert!(packed.contains("请继续完成最终 PR 审查"));
    }

    #[test]
    fn diag_log_ring_evicts_oldest_beyond_capacity() {
        let mut ring = DiagLogRing::new(16);
        for i in 0..25 {
            ring.push(DiagLevel::Info, "git", Some("SYNC"), format!("msg-{i}"), Vec::new());
        }
        let recs = ring.records();
        assert_eq!(recs.len(), 16);
        assert_eq!(recs.first().unwrap().message, "msg-9");
        assert_eq!(recs.last().unwrap().message, "msg-24");
    }

    #[test]
    fn generic_workspace_alias_preserves_saved_project_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        let chats = tmp.path().join("chats");
        std::fs::create_dir(&workspace).unwrap();
        let mut registry = ProjectRegistry::load_or_seed(&chats, &workspace);
        let original = registry.ensure_project(&workspace);
        registry.save(&chats).unwrap();
        let restored = ProjectRegistry::load_or_seed(&chats, &workspace);
        let project = &restored.projects[0];
        assert_eq!(project.id, original.id);
        assert_eq!(project.root_path, original.root_path);
        assert_eq!(project.name, "workspace");
        assert_eq!(project_display_name(&project.name), "Kwork");
        assert_eq!(project_display_name("Work Space"), "Kwork");
        assert_eq!(project_display_name("Workspace Notes"), "Workspace Notes");
    }

    #[test]
    fn project_registry_and_managed_worktree_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo_dir = tmp.path().join("sample-project");
        let store_dir = tmp.path().join("chat-store");
        std::fs::create_dir_all(&repo_dir).unwrap();
        std::fs::create_dir_all(&store_dir).unwrap();

        let status = Command::new("git")
            .current_dir(&repo_dir)
            .args(["init", "-b", "main"])
            .status()
            .unwrap();
        assert!(status.success());
        Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.email", "test@kar.local"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.name", "Kar Test"])
            .status()
            .unwrap();
        std::fs::write(repo_dir.join("项目 README.md"), "# Sample\n").unwrap();
        std::fs::create_dir_all(repo_dir.join(".github")).unwrap();
        std::fs::create_dir_all(repo_dir.join("src")).unwrap();
        std::fs::write(repo_dir.join("src/main.rs"), "fn main() {}\n").unwrap();

        // Hidden dot-folders are excluded by default so `src` comes first!
        let default_tree = scan_workspace_tree(&repo_dir, 64);
        assert_eq!(default_tree.first().map(|t| t.name.as_str()), Some("src"));
        assert!(!default_tree.iter().any(|t| t.name == ".github"));
        let hidden_tree = scan_workspace_tree_with_options(&repo_dir, 64, true);
        assert!(hidden_tree.iter().any(|t| t.name == ".github"));

        let repo = Repository::open(&repo_dir).unwrap();
        repo.stage(&[PathBuf::from("项目 README.md"), PathBuf::from("src/main.rs")])
            .unwrap();
        repo.commit("chore: init").unwrap();

        // Stage + Unstage roundtrip
        std::fs::write(repo_dir.join("项目 README.md"), "# Updated\n").unwrap();
        repo.stage(&[PathBuf::from("项目 README.md")]).unwrap();
        let staged = discover_git_workspace(&repo_dir);
        assert_eq!(staged.staged_count(), 1);
        let readme = staged
            .files
            .iter()
            .find(|file| file.path == Path::new("项目 README.md"))
            .unwrap();
        assert_eq!((readme.added_lines, readme.deleted_lines), (1, 1));
        unstage_git_paths(&repo_dir, &[PathBuf::from("项目 README.md")]).unwrap();
        assert_eq!(discover_git_workspace(&repo_dir).staged_count(), 0);
        repo.stage(&[PathBuf::from("项目 README.md")]).unwrap();
        repo.commit("docs: update").unwrap();

        // Create isolated worktree and map back to parent StudioProject
        let mut reg = ProjectRegistry::load_or_seed(&store_dir, &repo_dir);
        let chat_id = Uuid::new_v4();
        let wt_meta =
            create_isolated_worktree_for_chat(&repo_dir, &store_dir, Uuid::new_v4()).unwrap();
        assert!(wt_meta.worktree_path.is_dir());
        assert!(wt_meta.branch.starts_with("kar/wt-"));
        reg.chat_worktrees.insert(chat_id, wt_meta.clone());
        reg.toggle_pin(chat_id);
        reg.save(&store_dir).unwrap();

        let reloaded = ProjectRegistry::load_or_seed(&store_dir, &repo_dir);
        assert!(reloaded.is_pinned(chat_id));
        let matched = reloaded
            .project_for_chat(&wt_meta.worktree_path, chat_id)
            .expect("worktree maps to parent project");
        assert_eq!(matched.name, "sample-project");
    }
}

#[cfg(test)]
mod context_pack_limits {
    use super::*;
    #[test]
    fn long_multibyte_handoff_note_stays_within_the_byte_limit() {
        for size in [1024, 4096, 16 * 1024] {
            let packed = build_portable_handoff_context(
                "codex",
                "claude",
                &[],
                &"继续🙂".repeat(20_000),
                size,
            );
            assert!(packed.len() <= size);
            assert!(packed.ends_with('…'));
        }
    }
}
