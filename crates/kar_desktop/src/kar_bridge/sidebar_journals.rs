//! React to journal/receipt changes from other windows without replaying the entire chat store.
use super::{KarBridgeEvent, WorkerState, load_persisted_titles, read_chat_history_resilient};
use crate::{client_store::THREAD_RECEIPTS_FILE, types::ChatListEntry};
use kar::chat::TurnStatus;
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileStamp {
    modified: SystemTime,
    len: u64,
}

impl FileStamp {
    pub(super) fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            modified: metadata.modified()?,
            len: metadata.len(),
        })
    }
}

fn journal_id(path: &Path) -> Option<Uuid> {
    match path.extension()?.to_str()? {
        "karchat" | "kwrok-turn" => path.file_stem()?.to_str()?.parse().ok(),
        _ => None,
    }
}

/// A long-lived writer can keep FS notification delivery for its journal pending until close.
/// Publish a small, atomically closed hint only at actual Kar turn transitions.
pub(super) fn signal_turn(dir: &Path, id: Uuid) {
    let name = format!("{id}.kwrok-turn");
    if let Err(error) = crate::client_store::write_desktop_preference(dir, &name, "") {
        log::debug!("Could not signal turn activity for {id}: {error}");
    }
}

#[derive(Default)]
struct Pending {
    journals: HashSet<Uuid>,
    receipts: bool,
    rescan: bool,
}

#[derive(Default)]
pub(super) struct Changes {
    watcher: Option<notify::RecommendedWatcher>,
    pending: Arc<Mutex<Pending>>,
    settling: HashMap<Uuid, FileStamp>,
    last_fallback: Option<Instant>,
    deferred_selected: Option<Uuid>,
}

impl Changes {
    pub(super) fn watch(dir: &Path) -> Self {
        let mut changes = Self::default();
        let queue = changes.pending.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let mut pending = queue.lock().unwrap_or_else(|e| e.into_inner());
            match res {
                Ok(e) if e.need_rescan() => pending.rescan = true,
                Ok(e) if !matches!(e.kind, EventKind::Access(_)) => {
                    for path in e.paths {
                        if let Some(id) = journal_id(&path) {
                            pending.journals.insert(id);
                        } else if path
                            .file_name()
                            .is_some_and(|name| name == THREAD_RECEIPTS_FILE)
                        {
                            pending.receipts = true;
                        }
                    }
                }
                Err(_) => pending.rescan = true,
                _ => {}
            }
        })
        .and_then(|mut watcher| {
            watcher.watch(dir, RecursiveMode::NonRecursive)?;
            Ok(watcher)
        });
        match watcher {
            Ok(watcher) => changes.watcher = Some(watcher),
            Err(e) => log::warn!("Could not watch chat store for sidebar updates: {e}"),
        }
        // The UI may have read the local watermarks before this watch was installed.
        changes
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .receipts = true;
        changes
    }

    fn drain(&mut self) -> Pending {
        let mut work = std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()));
        if self.watcher.is_none()
            && self
                .last_fallback
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(15))
        {
            self.last_fallback = Some(Instant::now());
            work.rescan = true;
        }
        work.journals.extend(self.settling.keys().copied());
        work
    }
}

impl WorkerState {
    pub(super) fn sync_sidebar_journals(&mut self) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let mut work = self.sidebar_changes.drain();
        let subscribed = self.selected_chat_id.filter(|_| {
            self.remote_session.is_none()
                && self
                    .feed_cancel
                    .as_ref()
                    .is_some_and(|feed| feed.receiver_count() > 0)
        });
        // Some file systems defer modify events while a producer holds its journal open.
        // Probe only known running background chats until a terminal status arrives.
        work.journals
            .extend(self.chat_list_cache.iter().filter_map(|(id, (_, _, row))| {
                (Some(*id) != subscribed && row.last_status == Some(TurnStatus::Running))
                    .then_some(*id)
            }));
        if self.sidebar_changes.deferred_selected != subscribed
            && let Some(previous) = self.sidebar_changes.deferred_selected.take()
        {
            work.journals.insert(previous);
        }
        // An OS event queue overflow (or unavailable watcher) calls for metadata reconciliation.
        if work.rescan {
            work.receipts = true;
            if let Ok(files) = fs::read_dir(&self.store_dir) {
                work.journals
                    .extend(files.flatten().filter_map(|file| journal_id(&file.path())));
                work.journals.extend(self.chat_list_cache.keys().copied());
            }
        }
        if work.receipts
            && let Ok(bytes) = fs::read(self.store_dir.join(THREAD_RECEIPTS_FILE))
        {
            self.emit(KarBridgeEvent::ReadReceiptsChanged {
                dir: self.store_dir.clone(),
                bytes,
            });
        }

        let mut changed = Vec::new();
        let mut removed = Vec::new();
        let mut titles = None;
        for id in work.journals {
            let path = self.store_dir.join(format!("{id}.karchat"));
            let stamp = match FileStamp::read(&path) {
                Ok(stamp) => stamp,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    self.sidebar_changes.settling.remove(&id);
                    // Also clean a stale desktop hint left by a deletion in another client.
                    let _ = fs::remove_file(self.store_dir.join(format!("{id}.kwrok-turn")));
                    if self.chat_list_cache.remove(&id).is_some() {
                        removed.push(id);
                    }
                    continue;
                }
                Err(_) => continue,
            };
            if let Some((old, _, row)) = self.chat_list_cache.get(&id) {
                if *old == Some(stamp) {
                    self.sidebar_changes.settling.remove(&id);
                    continue;
                }
                if subscribed == Some(id) {
                    // Its live feed already updates the selected row. Reconcile the cached
                    // summary once when this window switches away, including a paused stream.
                    self.sidebar_changes.deferred_selected = Some(id);
                    self.sidebar_changes.settling.remove(&id);
                    continue;
                }
                // Start the ring at the first write. Coalesce streaming writes and project
                // the result after a quiet interval instead of replaying on every text delta.
                if row.last_status == Some(TurnStatus::Running)
                    && self.sidebar_changes.settling.insert(id, stamp) != Some(stamp)
                {
                    continue;
                }
            }
            self.sidebar_changes.settling.remove(&id);
            let Ok(history) = read_chat_history_resilient(store, &self.store_dir, id) else {
                continue;
            };
            let custom = titles
                .get_or_insert_with(|| load_persisted_titles(&self.store_dir))
                .get(&id)
                .cloned();
            let row = ChatListEntry::from_info_and_turns_with_title(
                history.chat,
                &history.turns,
                custom.as_deref(),
            );
            self.chat_list_cache
                .insert(id, (Some(stamp), custom, row.clone()));
            changed.push(row);
        }
        if !changed.is_empty() || !removed.is_empty() {
            self.emit(KarBridgeEvent::ChatsPatched {
                dir: self.store_dir.clone(),
                changed,
                removed,
            });
        }
    }
}
