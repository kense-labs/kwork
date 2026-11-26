//! Select one file from an immutable Kar turn checkpoint.
use kar::chat::ChatStore;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewKey {
    pub chat: Uuid,
    pub turn: Uuid,
    pub index: usize,
    pub path: PathBuf,
}

pub(crate) struct FileReview {
    pub key: ReviewKey,
    pub patch: Option<Result<Arc<Vec<String>>, String>>,
}

fn path_matches(token: &str, prefix: &[u8], path: &Path) -> bool {
    let token = token.trim_end_matches('\t');
    let Ok((decoded, consumed)) = gix_quote::ansi_c::undo(token.as_bytes().into()) else {
        return false;
    };
    let bytes: &[u8] = decoded.as_ref().as_ref();
    #[cfg(windows)]
    let normalized = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let expected = normalized.as_bytes();
    #[cfg(not(windows))]
    let expected = path.as_os_str().as_encoded_bytes();
    consumed == token.len() && bytes.strip_prefix(prefix) == Some(expected)
}

fn patch_for_path<'a>(diff: &'a str, path: &Path) -> Option<&'a str> {
    let mut starts = diff
        .match_indices("\ndiff --git ")
        .map(|(offset, _)| offset + 1)
        .chain(std::iter::once(diff.len()));
    let mut start = 0;
    while start < diff.len() {
        let end = starts.next()?;
        let section = &diff[start..end];
        start = end;
        let mut lines = section.lines();
        let Some(header) = lines.next().and_then(|l| l.strip_prefix("diff --git ")) else {
            continue;
        };
        if lines
            .take_while(|l| !l.starts_with("@@") && !l.starts_with("GIT binary patch"))
            .any(|line| {
                [
                    ("--- ", b"a/".as_slice()),
                    ("+++ ", b"b/"),
                    ("rename from ", b""),
                    ("rename to ", b""),
                    ("copy from ", b""),
                    ("copy to ", b""),
                ]
                .iter()
                .any(|(marker, prefix)| {
                    line.strip_prefix(marker)
                        .is_some_and(|name| path_matches(name, prefix, path))
                })
            })
        {
            return Some(section);
        }
        // Binary files and mode-only changes can have neither --- nor +++.
        // Match both sides so spaces in a Git filename remain unambiguous.
        if header.match_indices(" ").any(|(i, _)| {
            path_matches(&header[..i], b"a/", path) && path_matches(&header[i + 1..], b"b/", path)
        }) {
            return Some(section);
        }
    }
    None
}

pub(crate) fn file_patch(store: &ChatStore, key: &ReviewKey) -> Result<Vec<String>, String> {
    let history = store.history(key.chat).map_err(|err| err.to_string())?;
    if history
        .turns
        .get(key.index)
        .is_none_or(|turn| turn.id != key.turn)
    {
        return Err("这轮对话的历史记录已改变，请重新选择文件。".into());
    }
    let diff = store
        .turn_diff(key.chat, key.index, key.index + 1, false)
        .map_err(|err| err.to_string())?;
    let mut section = patch_for_path(&diff.diff, &key.path);
    if section.is_none() {
        section = diff.repositories.iter().flatten().find_map(|repo| {
            let relative = key.path.strip_prefix(&repo.relative_path).ok()?;
            patch_for_path(&repo.diff, relative)
        });
    }
    let section = section.ok_or_else(|| {
        if diff.diff.contains("\n[truncated]") {
            "本轮差异超出检查点输出上限，此文件的差异不可用。"
        } else {
            "检查点未提供此文件的差异。"
        }
        .to_owned()
    })?;
    // File identity is already visible in the header. Retain binary / rename / mode metadata.
    let mut lines = section.lines().skip(1).peekable();
    let mut result = Vec::new();
    while lines.peek().is_some_and(|l| !l.starts_with("@@")) {
        let line = lines.next().unwrap();
        if !line.starts_with("index ") && !line.starts_with("--- ") && !line.starts_with("+++ ") {
            result.push(line.to_owned());
        }
    }
    result.extend(lines.map(str::to_owned));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_only_the_named_file_including_deletions_and_quoted_names() {
        let patch = "diff --git a/one b/one\n+++ b/one\n@@ -0 +1 @@\n+one\ndiff --git \"a/a\\tfile\" \"b/a\\tfile\"\n--- \"a/a\\tfile\"\n+++ /dev/null\n@@ -1 +0 @@\n-deleted\ndiff --git a/my data b/my data\nBinary files a/my data and b/my data differ\n";
        let deleted = patch_for_path(patch, Path::new("a\tfile")).unwrap();
        assert!(
            deleted.contains("-deleted")
                && !deleted.contains("+one")
                && !deleted.contains("Binary files")
        );
        assert!(
            patch_for_path(patch, Path::new("my data"))
                .unwrap()
                .contains("Binary files")
        );
        assert!(patch_for_path(patch, Path::new("absent")).is_none());
    }
}
