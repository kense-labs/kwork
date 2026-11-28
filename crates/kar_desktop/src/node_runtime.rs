//! Managed user-space Node.js LTS runtime & PATH discovery for npm-distributed CLI agents
//! (`DeepSeek Harness`, `CodeBuddy Code`, `Qwen`, `Pi`, and ACP registry npm packages).
//!
//! When macOS launches `Kwork.app` from Dock/Finder, `PATH` is minimal (`/usr/bin:/bin:...`)
//! and a fresh machine may not have Node.js/npm installed at all. Rather than failing with
//! `unsupported operation: npm`, Kwork discovers existing Node version managers (`nvm`, `fnm`,
//! `volta`, `mise`, `asdf`, Homebrew) and, if `npm` is still absent, provisions an isolated,
//! SHA-256-verified Node.js LTS toolchain under `~/.local/share/kwork/runtimes/node/`.

use std::{
    ffi::{OsStr, OsString},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result, bail, ensure};

const NODE_VERSION: &str = "v22.14.0";

static PROVISION_LOCK: Mutex<()> = Mutex::new(());

fn npm_bin_name() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

fn node_bin_name() -> &'static str {
    if cfg!(windows) { "node.exe" } else { "node" }
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

pub(crate) fn managed_node_root() -> Option<PathBuf> {
    std::env::home_dir()
        .map(|home| home.join(".local/share/kwork/runtimes/node").join(NODE_VERSION))
}

pub(crate) fn managed_node_bin_dir() -> Option<PathBuf> {
    let root = managed_node_root()?;
    Some(if cfg!(windows) {
        root
    } else {
        root.join("bin")
    })
}

/// Collect candidate Node/CLI bin directories from common developer version managers.
pub(crate) fn extra_user_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".bun/bin"),
        home.join(".opencode/bin"),
        home.join(".kimi-code/bin"),
        home.join(".grok/bin"),
        home.join(".volta/bin"),
        home.join(".fnm/aliases/default/bin"),
        home.join("Library/Application Support/fnm/aliases/default/bin"),
        home.join(".asdf/shims"),
        home.join(".mise/shims"),
        home.join(".local/share/mise/shims"),
        home.join(".npm-global/bin"),
    ];
    if let Some(nvm_nodes) = newest_subdirs(&home.join(".nvm/versions/node"), "bin") {
        out.extend(nvm_nodes);
    }
    if let Some(managed) = managed_node_bin_dir() {
        out.push(managed);
    }
    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from(
            "/Applications/Cursor.app/Contents/Resources/app/bin",
        ));
    }
    out
}

fn newest_subdirs(parent: &Path, suffix: &str) -> Option<Vec<PathBuf>> {
    let entries = std::fs::read_dir(parent).ok()?;
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path().join(suffix))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.reverse();
    Some(dirs)
}

pub(crate) fn find_program_in_path(program: &str, path_os: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_os)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable_file(candidate))
}

pub(crate) fn has_npm_in_path(path_os: &OsStr) -> bool {
    find_program_in_path(npm_bin_name(), path_os).is_some()
        && find_program_in_path(node_bin_name(), path_os).is_some()
}

/// Ensure a user-writable global npm prefix directory (`~/.local`) exists and return it
/// when no explicit `NPM_CONFIG_PREFIX` is configured or when the default prefix isn't writable.
pub(crate) fn user_writable_npm_prefix() -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    let prefix = home.join(".local");
    let bin = prefix.join("bin");
    let lib = prefix.join("lib/node_modules");
    let _ = std::fs::create_dir_all(&bin);
    let _ = std::fs::create_dir_all(&lib);
    Some(prefix)
}

/// Official Node.js v22.14.0 LTS release archive name, extracted top folder, and SHA-256 digest
/// from `https://nodejs.org/dist/v22.14.0/SHASUMS256.txt`.
fn release_artifact() -> Option<(&'static str, &'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some((
            "node-v22.14.0-darwin-arm64.tar.gz",
            "node-v22.14.0-darwin-arm64",
            "e9404633bc02a5162c5c573b1e2490f5fb44648345d64a958b17e325729a5e42",
        )),
        ("macos", "x86_64") => Some((
            "node-v22.14.0-darwin-x64.tar.gz",
            "node-v22.14.0-darwin-x64",
            "6698587713ab565a94a360e091df9f6d91c8fadda6d00f0cf6526e9b40bed250",
        )),
        ("linux", "aarch64") => Some((
            "node-v22.14.0-linux-arm64.tar.gz",
            "node-v22.14.0-linux-arm64",
            "8cf30ff7250f9463b53c18f89c6c606dfda70378215b2c905d0a9a8b08bd45e0",
        )),
        ("linux", "x86_64") => Some((
            "node-v22.14.0-linux-x64.tar.gz",
            "node-v22.14.0-linux-x64",
            "9d942932535988091034dc94cc5f42b6dc8784d6366df3a36c4c9ccb3996f0c2",
        )),
        ("windows", "x86_64") => Some((
            "node-v22.14.0-win-x64.zip",
            "node-v22.14.0-win-x64",
            "55b639295920b219bb2acbcfa00f90393a2789095b7323f79475c9f34795f217",
        )),
        _ => None,
    }
}

/// Ensure `node` and `npm` are available in `path_os`; if missing, download and verify the
/// portable Node.js LTS runtime under `~/.local/share/kwork/runtimes/node/`.
pub(crate) fn ensure_node_and_npm(path_os: &OsStr) -> Result<PathBuf> {
    let _guard = PROVISION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = user_writable_npm_prefix();
    if let Some(npm) = find_program_in_path(npm_bin_name(), path_os)
        && find_program_in_path(node_bin_name(), path_os).is_some()
        && let Some(parent) = npm.parent()
    {
        return Ok(parent.to_path_buf());
    }
    let managed_bin = managed_node_bin_dir().context("未找到用户主目录")?;
    if is_executable_file(&managed_bin.join(node_bin_name()))
        && is_executable_file(&managed_bin.join(npm_bin_name()))
    {
        return Ok(managed_bin);
    }

    let (archive_name, folder_name, expected_sha256) =
        release_artifact().context("当前系统架构暂不支持自动准备 Node.js 运行环境，请手动安装 Node.js")?;
    let target_dir = managed_node_root().context("未找到用户主目录")?;
    let parent_dir = target_dir
        .parent()
        .context("无效的运行环境目录")?
        .to_path_buf();
    std::fs::create_dir_all(&parent_dir)?;

    let urls = [
        format!("https://npmmirror.com/mirrors/node/{NODE_VERSION}/{archive_name}"),
        format!("https://nodejs.org/dist/{NODE_VERSION}/{archive_name}"),
    ];

    let tmp_archive = tempfile::NamedTempFile::new_in(&parent_dir)?;
    let mut downloaded = false;
    let mut last_err = String::new();

    for url in &urls {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(90)))
            .build()
            .new_agent();
        match agent.get(url).call() {
            Ok(resp) => {
                let mut reader = resp.into_body().into_reader().take(128 << 20);
                let mut file = std::fs::File::create(tmp_archive.path())?;
                let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
                let mut buf = [0u8; 65536];
                let mut copy_ok = true;
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            digest.update(&buf[..n]);
                            if let Err(e) = file.write_all(&buf[..n]) {
                                last_err = e.to_string();
                                copy_ok = false;
                                break;
                            }
                        }
                        Err(e) => {
                            last_err = e.to_string();
                            copy_ok = false;
                            break;
                        }
                    }
                }
                if !copy_ok {
                    continue;
                }
                let _ = file.sync_all();
                let actual: String = digest
                    .finish()
                    .as_ref()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                if actual == expected_sha256 {
                    downloaded = true;
                    break;
                }
                last_err = format!("安装包校验和不匹配 ({url})");
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }

    if !downloaded {
        bail!("自动下载 Node.js 运行环境失败：{last_err}。你也可以手动运行 `brew install node` 后重试。");
    }

    let staging = tempfile::Builder::new()
        .prefix("node-stage-")
        .tempdir_in(&parent_dir)?;

    #[cfg(not(windows))]
    {
        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(tmp_archive.path())
            .arg("-C")
            .arg(staging.path())
            .status()
            .context("解压 Node.js 运行环境失败（无法启动 tar）")?;
        ensure!(status.success(), "解压 Node.js 运行环境失败");
    }
    #[cfg(windows)]
    {
        let file = std::fs::File::open(tmp_archive.path())?;
        let mut zip = zip::ZipArchive::new(file)?;
        zip.extract(staging.path())?;
    }

    let unpacked = staging.path().join(folder_name);
    ensure!(unpacked.is_dir(), "Node.js 压缩包目录结构异常");
    if target_dir.exists() {
        let _ = std::fs::remove_dir_all(&target_dir);
    }
    std::fs::rename(&unpacked, &target_dir)
        .context("激活本地 Node.js 运行环境目录失败")?;

    ensure!(
        is_executable_file(&managed_bin.join(node_bin_name()))
            && is_executable_file(&managed_bin.join(npm_bin_name())),
        "Node.js 运行环境准备完成但未检测到可执行文件"
    );
    Ok(managed_bin)
}

/// Write a `KEY=VALUE` credential entry into `~/.dsh/.env` (used by DeepSeek Harness `dsh`).
pub(crate) fn save_dsh_api_key(api_key: &str) -> Result<()> {
    let key = api_key.trim();
    ensure!(!key.is_empty(), "API Key 不能为空");
    ensure!(
        !key.contains(['\n', '\r', '\0']),
        "API Key 包含无效换行或控制字符"
    );
    let home = std::env::var_os("DSH_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".dsh")))
        .context("未找到用户目录")?;
    std::fs::create_dir_all(&home)?;
    let env_path = home.join(".env");
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("DEEPSEEK_API_KEY=")
                && !trimmed.starts_with("export DEEPSEEK_API_KEY=")
        })
        .map(str::to_owned)
        .collect();
    lines.push(format!("DEEPSEEK_API_KEY={key}"));
    let mut content = lines.join("\n");
    content.push('\n');
    std::fs::write(&env_path, content.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Save a quick OpenAI-compatible provider block to Kwork's built-in runtime config (`~/.aster/config.acl`).
pub(crate) fn save_builtin_model_config(
    provider_slug: &str,
    base_url: &str,
    api_key: &str,
    model_id: &str,
) -> Result<PathBuf> {
    let path = crate::aster::config_path().context("未找到内置模型配置文件路径")?;
    let slug: String = provider_slug
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let slug = if slug.is_empty() {
        "custom".to_owned()
    } else {
        slug.to_ascii_lowercase()
    };
    let base_url = base_url.trim();
    let api_key = api_key.trim();
    let model_id = model_id.trim();
    ensure!(!base_url.is_empty(), "请填写服务地址 (Base URL)");
    ensure!(!api_key.is_empty(), "请填写 API Key");
    ensure!(!model_id.is_empty(), "请填写默认模型名称");
    for field in [base_url, api_key, model_id] {
        ensure!(
            !field.contains(['"', '\\', '\n', '\r']),
            "配置字段不能包含引号、反斜杠或换行"
        );
    }
    let clean_model = model_id
        .strip_prefix(&format!("{slug}/"))
        .unwrap_or(model_id);
    let acl = format!(
        "default_model = \"{slug}/{clean_model}\"\n\nproviders \"{slug}\" {{\n  base_url = \"{base_url}\"\n  api_key = \"{api_key}\"\n  models \"{clean_model}\" {{\n  }}\n}}\n"
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, acl.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(path)
}

/// Ensure `~/.aster/config.acl` exists on disk (writing a commented starter template if absent)
/// so revealing it in Finder/Explorer never fails silently.
pub(crate) fn ensure_builtin_config_template() -> Option<PathBuf> {
    let path = crate::aster::config_path()?;
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let starter = "\
# Kwork 默认运行环境模型配置
# 取消下方注释并填入你的 API Key 即可生效：
#
# default_model = \"deepseek/deepseek-chat\"
#
# providers \"deepseek\" {
#   base_url = \"https://api.deepseek.com/v1\"
#   api_key = \"sk-...\"
#   models \"deepseek-chat\" {}
#   models \"deepseek-reasoner\" {}
# }
";
        let _ = std::fs::write(&path, starter);
    }
    Some(path)
}

/// Translate low-level Kar setup/install/login English diagnostics into clear, actionable Chinese.
pub(crate) fn humanize_agent_error(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "unsupported operation: npm" || lower == "npm" {
        return "未检测到可用的 Node.js / npm 环境；请检查网络连接以自动准备运行环境，或运行 `brew install node` 后重试".into();
    }
    if lower.contains("unsupported operation: curl") || lower == "curl" {
        return "系统缺少 `curl` 下载工具，请先安装命令行工具后重试".into();
    }
    if lower.contains("not writable") || lower.contains("permission denied") {
        return format!("安装目录缺少当前用户写入权限（{trimmed}）");
    }
    if lower.contains("could not start a verified native executable") {
        return "未能启动已验证的命令行程序；请确认该智能体 CLI 已安装（如 `~/.local/bin`），或点击“重新检测”".into();
    }
    if lower.contains("native sign-in timed out") {
        return "浏览器登录等待超时，请重新点击登录并在浏览器中完成确认".into();
    }
    if lower.contains("ended without reporting a successful login")
        || lower.contains("ended without a valid sign-in response")
    {
        return "授权尚未完成，请在打开的浏览器页面确认登录后重试".into();
    }
    if lower.contains("refused the sign-in request") {
        return "智能体拒绝了本次登录请求，请检查网络或客户端状态后重试".into();
    }
    if lower.contains("without a discoverable executable") {
        if let Some((_, tail)) = trimmed.split_once(':') {
            let last_line = tail
                .lines()
                .rev()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("未找到安装后的可执行文件");
            return format!("安装命令已结束但未检测到可执行程序：{last_line}");
        }
        return "安装命令已结束，但未在标准路径中检测到可执行程序".into();
    }
    trimmed.to_owned()
}

pub(crate) fn build_npm_env_overrides(
    use_mirror: bool,
) -> Vec<(OsString, Option<OsString>)> {
    let mut out = Vec::new();
    if let Some(prefix) = user_writable_npm_prefix() {
        out.push((
            OsString::from("NPM_CONFIG_PREFIX"),
            Some(prefix.into_os_string()),
        ));
    }
    if use_mirror {
        out.push((
            OsString::from("NPM_CONFIG_REGISTRY"),
            Some(OsString::from("https://registry.npmmirror.com")),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_npm_and_cursor_executable_errors() {
        let npm_msg = humanize_agent_error("unsupported operation: npm");
        assert!(npm_msg.contains("Node.js / npm"));

        let cursor_msg = humanize_agent_error("Could not start a verified native executable");
        assert!(cursor_msg.contains("未能启动已验证的命令行程序"));
    }
}
