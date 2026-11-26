//! Read state belongs to the desktop user, including chats outside the active project's catalog.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use crate::client_store::THREAD_RECEIPTS_FILE as KEY;

#[derive(Serialize, Deserialize)]
struct Receipts {
    since_ms: u64,
    read: BTreeMap<Uuid, u64>,
}

pub(super) struct ThreadReadReceipts {
    dir: PathBuf,
    state: Receipts,
}

impl ThreadReadReceipts {
    pub(super) fn load(dir: &Path, now: u64) -> Self {
        let mut receipts = Self {
            dir: dir.to_owned(),
            state: Receipts {
                since_ms: now,
                read: BTreeMap::new(),
            },
        };
        if let Err(error) = receipts.update(None) {
            log::warn!("Could not initialize thread read receipts: {error}");
        }
        receipts
    }

    pub(super) fn unread(&self, chat: Uuid, completion: u64, visited: Option<u64>) -> bool {
        completion > self.state.since_ms
            && completion > self.state.read.get(&chat).copied().unwrap_or(0)
            && completion > visited.unwrap_or(0)
    }

    pub(super) fn refresh(&mut self, bytes: &[u8]) {
        match serde_json::from_slice::<Receipts>(bytes) {
            Ok(latest) => {
                // A queued watcher event can precede a read just acknowledged by this window.
                self.state.since_ms = latest.since_ms;
                for (id, time) in latest.read {
                    self.state
                        .read
                        .entry(id)
                        .and_modify(|at| *at = (*at).max(time))
                        .or_insert(time);
                }
            }
            Err(error) => log::warn!("Could not refresh thread read receipts: {error}"),
        }
    }

    pub(super) fn acknowledge(&mut self, chat: Uuid, completion: u64) {
        if let Err(error) = self.update(Some((chat, completion))) {
            log::warn!("Could not persist thread read receipt: {error}");
            self.state
                .read
                .entry(chat)
                .and_modify(|at| *at = (*at).max(completion))
                .or_insert(completion);
        }
    }

    fn update(&mut self, seen: Option<(Uuid, u64)>) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        // Different windows may finish or open different chats at the same time.
        let lock = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join("thread-read-receipts.lock"))?;
        lock.lock()?;
        let mut dirty = false;
        match fs::read(self.dir.join(KEY)) {
            Ok(bytes) => self.state = serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => dirty = true,
            Err(e) => return Err(e),
        }
        if let Some((id, completion)) = seen {
            let watermark = self.state.read.entry(id).or_default();
            if completion > *watermark {
                *watermark = completion;
                dirty = true;
            }
        }
        if dirty {
            crate::client_store::write_desktop_preference(
                &self.dir,
                KEY,
                &serde_json::to_string(&self.state).map_err(io::Error::other)?,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_threads_start_read_and_only_the_opened_reply_is_acknowledged() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut first = ThreadReadReceipts::load(dir.path(), 100);
        let mut second = ThreadReadReceipts::load(dir.path(), 130);
        assert!(!second.unread(a, 90, None));
        assert!(second.unread(a, 115, None));
        first.acknowledge(a, 115);
        second.acknowledge(b, 125);
        let reopened = ThreadReadReceipts::load(dir.path(), 150);
        assert!(!reopened.unread(a, 115, None));
        assert!(!reopened.unread(b, 125, None));
        assert!(reopened.unread(a, 160, None));
        assert!(!reopened.unread(a, 160, Some(160)));
    }

    #[test]
    fn queued_file_events_cannot_reopen_a_reply_already_read_in_either_window() {
        let dir = tempfile::tempdir().unwrap();
        let chat = Uuid::new_v4();
        let mut reader = ThreadReadReceipts::load(dir.path(), 100);
        let mut sidebar = ThreadReadReceipts::load(dir.path(), 100);
        reader.acknowledge(chat, 115);
        let older_notice = fs::read(dir.path().join(KEY)).unwrap();
        sidebar.refresh(&older_notice);
        assert!(!sidebar.unread(chat, 115, None));
        assert!(sidebar.unread(chat, 130, None));

        sidebar.acknowledge(chat, 130);
        sidebar.refresh(&older_notice);
        assert!(!sidebar.unread(chat, 130, None));
        reader.refresh(&fs::read(dir.path().join(KEY)).unwrap());
        assert!(!reader.unread(chat, 130, None));
    }
}
