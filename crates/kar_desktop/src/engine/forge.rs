//! Project-scoped forge reads and chat-bound publication through Kar's operation ledger.
use anyhow::{Context, Result, bail, ensure};
use axum::http::Method;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use uuid::Uuid;

use super::Engine;
use crate::{
    forge_devices_voice::{ForgeIssue, ForgePullRequest},
    prefs::PrMergeMethod,
};

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn pull(v: &Value) -> Result<ForgePullRequest> {
    Ok(ForgePullRequest {
        number: v["number"].as_u64().context("pull number")?,
        title: text(v, "title"),
        author: text(v, "author"),
        state: text(v, "state"),
        head_branch: text(v, "head_ref"),
        head_sha: text(v, "head_sha"),
        base_branch: text(v, "base_ref"),
        is_draft: v["draft"].as_bool().unwrap_or(false),
        checks_summary: String::new(),
        url: text(v, "url"),
        body: text(v, "body"),
    })
}

fn issue(v: &Value) -> Result<ForgeIssue> {
    let key = v["key"].as_str().context("issue key")?;
    Ok(ForgeIssue {
        id: format!("{}:{key}", text(v, "scope")),
        number_label: text(v, "displayKey"),
        source: text(v, "tracker"),
        title: text(v, "title"),
        state: text(&v["state"], "name"),
        author: v["author"]["name"]
            .as_str()
            .or_else(|| v["author"]["login"].as_str())
            .unwrap_or_default()
            .into(),
        labels: v["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l["name"].as_str().or_else(|| l.as_str()))
            .map(str::to_owned)
            .collect(),
        url: text(v, "url"),
        body: text(v, "body"),
    })
}

pub(crate) async fn read(
    engine: &Engine,
) -> (Vec<ForgePullRequest>, Vec<ForgeIssue>, Option<String>) {
    let project = engine.project_id();
    let path = format!("/projects/{project}/forge/query");
    let pulls = engine.request(Method::POST, &path, Some(json!({"op":"pulls"})));
    let issues = engine.request(
        Method::POST,
        "/forge/issues/list",
        Some(json!({"projectId":project,"state":"open","limit":100})),
    );
    let (pulls, issues) = tokio::join!(pulls, issues);
    let mut errors = Vec::new();
    let pulls = pulls
        .and_then(|v| {
            v.as_array()
                .context("missing pull list")?
                .iter()
                .filter(|v| !matches!(v["state"].as_str(), Some("closed" | "merged" | "declined")))
                .map(pull)
                .collect()
        })
        .unwrap_or_else(|e| {
            errors.push(format!("PR：{e}"));
            Vec::new()
        });
    let issues = issues
        .and_then(|v| {
            for e in v["errors"].as_array().into_iter().flatten() {
                errors.push(format!(
                    "{}：{}",
                    text(e, "projectTitle"),
                    text(e, "message")
                ));
            }
            v["entries"]
                .as_array()
                .context("missing issue list")?
                .iter()
                .map(issue)
                .collect()
        })
        .unwrap_or_else(|e| {
            errors.push(format!("议题：{e}"));
            Vec::new()
        });
    (
        pulls,
        issues,
        (!errors.is_empty()).then(|| errors.join(" · ")),
    )
}

async fn git_action(
    engine: &Engine,
    id: Uuid,
    chat: Uuid,
    action: &str,
    title: &str,
    body: &str,
    base: &str,
) -> Result<Value> {
    let path = format!("/projects/{}/git/actions", engine.project_id());
    engine
        .request(
            Method::POST,
            &path,
            Some(
                json!({"id":id,"chat":chat,"action":action,"title":title,"body":body,"base":base}),
            ),
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let state = engine
                .request(Method::GET, &format!("{path}/{id}"), None)
                .await?;
            if state["terminal"] == true {
                let terminal = state["events"]
                    .as_array()
                    .and_then(|events| events.last())
                    .context("Kar 未返回操作回执")?;
                ensure!(
                    terminal["kind"] == "action_finished",
                    "{}",
                    terminal["message"].as_str().unwrap_or("Git 操作未完成")
                );
                return Ok::<Value, anyhow::Error>(terminal["result"].clone());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .context("尚未确认推送结果，可使用同一操作重试")?
}

fn remote_urls(workspace: &Path, remote: &str, push: bool) -> Result<Vec<String>> {
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(workspace).args(["remote", "get-url"]);
    if push {
        git.args(["--push", "--all"]);
    }
    let output = git.args(["--", remote]).output()?;
    ensure!(output.status.success(), "无法读取分支的托管仓库");
    Ok(std::str::from_utf8(&output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect())
}

fn hosted_identity(raw: &str) -> Result<(String, String)> {
    let url = if let Ok(url) = url::Url::parse(raw) {
        url
    } else {
        let (host, path) = raw.split_once(':').context("无法识别托管仓库")?;
        ensure!(!host.contains('/'), "无效的托管仓库");
        url::Url::parse(&format!("ssh://{host}/{path}")).context("无效的托管仓库")?
    };
    ensure!(
        matches!(url.scheme(), "ssh" | "https" | "http" | "git"),
        "需要托管仓库作为推送目标"
    );
    let host = url.host_str().context("仓库缺少主机")?.to_ascii_lowercase();
    let repo = url.path().trim_matches('/').trim_end_matches(".git");
    ensure!(
        repo.split('/').count() >= 2
            && repo
                .split('/')
                .all(|s| !s.is_empty() && s != "." && s != ".."),
        "无效的仓库名称"
    );
    Ok((host, repo.to_owned()))
}

fn push_repository(workspace: &Path, remote: &str, expected_host: &str) -> Result<String> {
    let fetch = remote_urls(workspace, remote, false)?;
    let mut destinations = std::collections::BTreeSet::new();
    for url in remote_urls(workspace, remote, true)? {
        let is_local =
            Path::new(&url).is_absolute() || url.starts_with("file://") || !url.contains(':');
        let raw = if is_local {
            fetch.first().context("缺少托管仓库地址")?
        } else {
            &url
        };
        let (host, repo) = hosted_identity(raw)?;
        ensure!(
            host.eq_ignore_ascii_case(expected_host),
            "草稿 PR 的源仓库和目标仓库必须在同一主机"
        );
        destinations.insert(repo);
    }
    ensure!(destinations.len() == 1, "草稿 PR 需要唯一的推送仓库");
    Ok(destinations.into_iter().next().unwrap())
}

pub(crate) async fn create(
    engine: &Engine,
    workspace: &Path,
    operation: Uuid,
    chat: Uuid,
    title: &str,
    body: &str,
    base: &str,
    draft: bool,
) -> Result<String> {
    ensure!(
        !title.trim().is_empty() && !base.trim().is_empty(),
        "PR 标题和目标分支不能为空"
    );
    let result = if draft {
        let identity = engine
            .request(
                Method::POST,
                &format!("/projects/{}/forge/query", engine.project_id()),
                Some(json!({"op":"identity"})),
            )
            .await?;
        let host = identity["host"].as_str().context("托管服务未返回主机名")?;
        let push_id = Uuid::new_v5(&operation, b"push");
        let push = git_action(engine, push_id, chat, "push", title, body, base).await?;
        let target = &push["push"];
        let head = target["published_branch"]
            .as_str()
            .context("工作台未返回已推送分支")?;
        let remote = target["remote"]
            .as_str()
            .context("工作台未返回已推送仓库")?;
        let repository = push_repository(workspace, remote, host)?;
        engine.request(Method::POST,&format!("/projects/{}/forge/operations",engine.project_id()),
            Some(json!({"id":operation,"chat":chat,"op":"create_pull","title":title,"body":body,"base":base,
                "draft":true,"head":head,"head_repository":repository}))).await?
    } else {
        git_action(engine, operation, chat, "create_pr", title, body, base).await?["pr"]["pull"]
            .clone()
    };
    let number = result["number"]
        .as_u64()
        .context("未收到已创建 PR 的编号，请刷新核对")?;
    Ok(format!(
        "PR #{number} · {}",
        result["url"].as_str().unwrap_or(title)
    ))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MergeOutcome {
    Merged,
    Submitted,
}

pub(crate) async fn merge(
    engine: &Engine,
    operation: Uuid,
    chat: Uuid,
    number: u64,
    sha: &str,
    method: PrMergeMethod,
) -> Result<MergeOutcome> {
    ensure!(!sha.is_empty() && number > 0, "刷新 PR 以确认要合并的版本");
    let method = match method {
        PrMergeMethod::Squash => "squash",
        PrMergeMethod::Merge => "merge",
        PrMergeMethod::Rebase => "rebase",
    };
    let result = engine
        .request(
            Method::POST,
            &format!("/projects/{}/forge/operations", engine.project_id()),
            Some(json!({
                "id":operation,"chat":chat,"op":"pull_command","number":number,
                "command":{"action":"merge","expected_head_sha":sha,"method":method}
            })),
        )
        .await?;
    if result["merged"] == true {
        return Ok(MergeOutcome::Merged);
    }
    if result["submitted"] == true {
        let detail = engine
            .request(
                Method::POST,
                &format!("/projects/{}/forge/query", engine.project_id()),
                Some(json!({"op":"pull","number":number})),
            )
            .await;
        return Ok(match detail {
            Ok(pull)
                if pull["state"]
                    .as_str()
                    .is_some_and(|state| state.eq_ignore_ascii_case("merged")) =>
            {
                MergeOutcome::Merged
            }
            _ => MergeOutcome::Submitted,
        });
    }
    bail!("工作台尚未确认合并")
}

#[cfg(test)]
mod tests;
