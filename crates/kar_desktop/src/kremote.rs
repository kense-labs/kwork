//! Hand user-approved UU desktop invitations to the installed compatible viewer.
use url::Url;

use crate::ui_primitives::UiLocale;

pub(crate) enum LaunchError {
    InvalidInvitation,
    ClientMissing,
    CouldNotOpen,
}

impl LaunchError {
    pub(crate) fn message(&self, loc: UiLocale) -> &'static str {
        match self {
            Self::InvalidInvitation => loc.pick(
                "请粘贴包含 9 位设备 ID 的 UU 远程协助链接。",
                "Paste a UU Remote assistance link containing a nine-digit device ID.",
            ),
            Self::ClientMissing => loc.pick(
                "未找到兼容客户端。macOS 请安装 UU 远程，Linux 或 Windows 请安装 KRemote。",
                "Install UU Remote on macOS or KRemote on Linux or Windows.",
            ),
            Self::CouldNotOpen => {
                loc.pick("客户端未能打开。", "The desktop client could not open.")
            }
        }
    }
}

// The link can contain a one-time credential. Never log it, persist it, or echo it in errors.
fn invitation(input: &str) -> Option<Url> {
    let input = input.trim();
    if input.len() > 1024 || input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let uri = Url::parse(input).ok()?;
    if uri.scheme() != "uuremote"
        || !uri.host_str()?.eq_ignore_ascii_case("external")
        || !uri
            .path()
            .trim_end_matches('/')
            .eq_ignore_ascii_case("/assistance")
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.port().is_some()
        || uri.fragment().is_some()
    {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut valid_id = false;
    for (key, value) in uri.query_pairs() {
        if matches!(&*key, "deviceId" | "code" | "controlledLogin") && !seen.insert(key.clone()) {
            return None;
        }
        match &*key {
            "deviceId" => {
                let digits: String = value.chars().filter(|c| *c != ' ').collect();
                valid_id = digits.len() == 9 && digits.bytes().all(|b| b.is_ascii_digit());
            }
            "code" if value.len() > 256 || value.chars().any(char::is_control) => return None,
            _ => {}
        }
    }
    valid_id.then_some(uri)
}

pub(crate) fn open(invite: Option<&str>) -> Result<(), LaunchError> {
    let uri = invite
        .map(|value| invitation(value).ok_or(LaunchError::InvalidInvitation))
        .transpose()?;
    platform::open(uri.as_ref())
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    pub(super) fn open(invite: Option<&Url>) -> Result<(), LaunchError> {
        let workspace = NSWorkspace::sharedWorkspace();
        let uri = NSURL::URLWithString(&NSString::from_str(
            invite.map_or("uuremote://external/assistance", Url::as_str),
        ))
        .ok_or(LaunchError::InvalidInvitation)?;
        let app = workspace
            .URLForApplicationToOpenURL(&uri)
            .ok_or(LaunchError::ClientMissing)?;
        let target = if invite.is_some() { &uri } else { &app };
        workspace
            .openURL(target)
            .then_some(())
            .ok_or(LaunchError::CouldNotOpen)
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn executable() -> Option<PathBuf> {
        let name = format!("kremote{}", std::env::consts::EXE_SUFFIX);
        let mut dirs = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default();
        if cfg!(windows)
            && let Some(dir) = std::env::var_os("ProgramFiles")
        {
            dirs.push(PathBuf::from(dir).join("Kremote"));
        }
        dirs.into_iter()
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(&name))
            .find(|path| path.is_file())
    }

    pub(super) fn open(invite: Option<&Url>) -> Result<(), LaunchError> {
        let mut command = Command::new(executable().ok_or(LaunchError::ClientMissing)?);
        if let Some(invite) = invite {
            command.arg(invite.as_str());
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| LaunchError::CouldNotOpen)?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitations_preserve_optional_codes_and_accept_formatted_device_ids() {
        for uri in [
            "uuremote://external/assistance?deviceId=123456789",
            "uuremote://external/assistance?deviceId=123%20456%20789&code=a%2Bb&controlledLogin=1",
        ] {
            assert_eq!(invitation(uri).as_ref().map(Url::as_str), Some(uri));
        }
    }

    #[test]
    fn malformed_invitations_never_launch_a_client() {
        for uri in [
            "https://external/assistance?deviceId=123456789",
            "uuremote://bad/external/assistance?deviceId=123456789",
            "uuremote://name@external/assistance?deviceId=123456789",
            "uuremote://external/assistance?code=123",
            "uuremote://external/assistance?deviceId=12345",
            "uuremote://external/assistance?deviceId=123456789&code=a&code=b",
            "uuremote://external/assistance?deviceId=123456789&code=%0D",
        ] {
            assert!(matches!(
                open(Some(uri)),
                Err(LaunchError::InvalidInvitation)
            ));
        }
    }
}
