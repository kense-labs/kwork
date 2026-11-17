//! Bounded desktop upload staging for the ACP connection's attachment bridge.

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use tempfile::{NamedTempFile, TempDir};

pub(crate) const CHUNK_BYTES: usize = 256 * 1024;

pub(crate) struct PromptFiles {
    files: Mutex<HashMap<String, StagedFile>>,
    directory: TempDir,
}

pub(crate) struct StagedFile {
    file: NamedTempFile,
    session: String,
    name: String,
    size: u64,
    received: u64,
}

impl PromptFiles {
    pub(crate) fn new() -> Result<Self, String> {
        let directory = tempfile::Builder::new()
            .prefix("kwork-prompt-files-")
            .tempdir()
            .map_err(|error| format!("无法创建附件暂存目录：{error}"))?;
        Ok(Self {
            files: Mutex::default(),
            directory,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        self.directory.path()
    }

    pub(crate) fn begin(&self, session: String, name: String, size: u64) -> Result<String, String> {
        if session.is_empty() {
            return Err("请先打开会话再上传附件。".into());
        }
        let file = tempfile::Builder::new()
            .prefix("file-")
            .rand_bytes(16)
            .tempfile_in(self.path())
            .map_err(|error| format!("无法暂存附件：{error}"))?;
        let id = file
            .path()
            .file_name()
            .and_then(|leaf| leaf.to_str())
            .ok_or("附件暂存路径无效。")?
            .to_owned();
        self.files.lock().map_err(|_| "附件暂存不可用。")?.insert(
            id.clone(),
            StagedFile {
                file,
                session,
                name,
                size,
                received: 0,
            },
        );
        Ok(id)
    }

    pub(crate) fn append(
        &self,
        session: &str,
        id: &str,
        offset: u64,
        chunk: Vec<u8>,
    ) -> Result<(), String> {
        if chunk.is_empty() || chunk.len() > CHUNK_BYTES {
            return Err("附件上传分块无效。".into());
        }
        let mut files = self.files.lock().map_err(|_| "附件暂存不可用。")?;
        let file = files
            .get_mut(id)
            .filter(|file| file.session == session)
            .ok_or("附件暂存已失效，请重新发送。")?;
        let end = offset
            .checked_add(chunk.len() as u64)
            .ok_or("附件长度无效。")?;
        if offset != file.received || end > file.size {
            return Err("附件上传位置无效。".into());
        }
        if let Err(error) = file.file.as_file_mut().write_all(&chunk) {
            // A failed write may have persisted only part of the chunk.
            files.remove(id);
            return Err(format!("暂存附件失败：{error}"));
        }
        file.received = end;
        Ok(())
    }

    pub(crate) fn discard(&self, session: &str, id: &str) -> Result<(), String> {
        let mut files = self.files.lock().map_err(|_| "附件暂存不可用。")?;
        if files.get(id).is_some_and(|file| file.session == session) {
            files.remove(id);
        }
        Ok(())
    }

    // Only this connection may translate an upload ID to a file on disk. The
    // holder returned to the ACP command keeps the staging file alive until
    // the Host has consumed it, and deletes it on success or failure.
    pub(crate) fn assemble(
        &self,
        session: &str,
        prompt: &mut Value,
    ) -> Result<Vec<StagedFile>, String> {
        let blocks = prompt.as_array_mut().ok_or("消息内容无效。")?;
        let mut files = self.files.lock().map_err(|_| "附件暂存不可用。")?;
        let mut seen = HashSet::new();
        for block in blocks.iter().filter(|block| block["type"] == "file") {
            let id = block["uploadId"].as_str().ok_or("附件缺少上传标识。")?;
            let Some(file) = files.get(id).filter(|file| file.session == session) else {
                return Err("附件暂存已失效，请重新发送。".into());
            };
            if file.received != file.size || !seen.insert(id.to_owned()) {
                return Err("附件上传未完成或标识重复。".into());
            }
        }
        let mut held = Vec::with_capacity(seen.len());
        for block in blocks.iter_mut().filter(|block| block["type"] == "file") {
            let id = block["uploadId"].as_str().unwrap_or_default();
            let file = files.remove(id).ok_or("附件暂存已失效，请重新发送。")?;
            *block = json!({"type": "file", "stagingId": id, "name": file.name, "size": file.size});
            held.push(file);
        }
        Ok(held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_files_are_ordered_session_bound_and_consumed_once() {
        let uploads = PromptFiles::new().unwrap();
        let id = uploads
            .begin("owner".into(), "notes.bin".into(), 3)
            .unwrap();
        let path = uploads.path().join(&id);
        let mut prompt = json!([{"type":"text","text":"inspect"}, {"type":"file","uploadId":id}]);
        assert!(uploads.append("different", &id, 0, vec![0]).is_err());
        assert!(uploads.append("owner", &id, 1, vec![0]).is_err());
        uploads.append("owner", &id, 0, vec![0, 255]).unwrap();
        assert!(uploads.assemble("owner", &mut prompt).is_err());
        uploads.append("owner", &id, 2, vec![17]).unwrap();
        assert!(uploads.assemble("different", &mut prompt).is_err());
        let mut duplicate = json!([{"type":"file","uploadId":id}, {"type":"file","uploadId":id}]);
        assert!(uploads.assemble("owner", &mut duplicate).is_err());
        let mut replay = prompt.clone();
        let held = uploads.assemble("owner", &mut prompt).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [0, 255, 17]);
        assert_eq!(
            prompt[1],
            json!({"type":"file","stagingId":id,"name":"notes.bin","size":3})
        );
        assert!(uploads.assemble("owner", &mut replay).is_err());
        drop(held);
        assert!(!path.exists());
    }
}
