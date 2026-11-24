//! End-to-end acceptance tests for Kar Desktop:
//! 1. Real Kar (`ChatStore` + `ChatSession` + `codex`) send, single-writer lock, idempotent UUID retry,
//!    second client `HttpChatFeed` + `ChatReplica` mid-tool disconnect & cursor-exact reconnect,
//!    duplicate event ignoring, cursor-gap resync, and exact parity against `ChatStore::history`.
//! 2. Real Kantor `.docx`, `.xlsx`, `.pptx` open -> edit -> save copy -> close -> reopen verification.
//! 3. Demo Co-Editing three scenarios + mid-conflict revision change re-confirmation + disconnect isolation.

use std::path::PathBuf;
use std::time::Duration;

use kar::{
    ApprovalMode, SessionOptions,
    chat::{
        ChatCursor, ChatEvent, ChatReplica, ChatStore, ChatUpdate, HttpChatFeed, Item, TurnStatus,
        event_router,
    },
};
use kar_desktop::{
    coedit::{
        CoEditService, CoEditStatus, DemoCoEditEngine, LiveTargetState, ScenarioKey,
        SubmissionOutcome, UserResolutionChoice,
    },
    kar_bridge::{KarBridgeEvent, KarBridgeHandle, KarCommand, apply_http_event_with_gap_recovery},
    office_host::{
        materialize_coedit_demo_files, verify_docx_roundtrip, verify_pptx_roundtrip,
        verify_xlsx_roundtrip,
    },
    types::{ChatAccessMode, FeedConnectionState},
};
use tokio::sync::oneshot;
use uuid::Uuid;

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests/corpus")
}

#[test]
fn office_three_formats_open_edit_save_close_reopen() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let work_dir = tmp.path();

    // Copy real corpus samples into isolated test copies (never touch originals!)
    let docx_copy = work_dir.join("minutes_working.docx");
    let docx_saved = work_dir.join("minutes_saved.docx");
    std::fs::copy(corpus_root().join("docx/minutes.docx"), &docx_copy).expect("copy docx");

    let xlsx_copy = work_dir.join("formulas_working.xlsx");
    let xlsx_saved = work_dir.join("formulas_saved.xlsx");
    std::fs::copy(corpus_root().join("xlsx/01_formulas.xlsx"), &xlsx_copy).expect("copy xlsx");

    let pptx_copy = work_dir.join("deck_working.pptx");
    let pptx_saved = work_dir.join("deck_saved.pptx");
    std::fs::copy(corpus_root().join("pptx/deck.pptx"), &pptx_copy).expect("copy pptx");

    // 1. Word (.docx): edit paragraph #2, save copy, close, reopen, verify edited & unedited paragraphs
    let word_report = verify_docx_roundtrip(
        &docx_copy,
        &docx_saved,
        2,
        "Decision: renewed vendor contract through 2027 (verified via Kantor DocsEngine).",
    )
    .expect("docx roundtrip");
    println!("[PASS DOCX] {} | {}", word_report.edited_summary, word_report.unedited_check);

    // 2. Excel (.xlsx): write inputs + formula =SUM(H2:H3)*2, verify live calc 800, save copy, close, reopen
    let excel_report = verify_xlsx_roundtrip(&xlsx_copy, &xlsx_saved).expect("xlsx roundtrip");
    println!("[PASS XLSX] {} | {}", excel_report.edited_summary, excel_report.unedited_check);

    // 3. PPT (.pptx): edit shape text & coordinates (4.25cm, 3.60cm), save copy, close, reopen
    let ppt_report = verify_pptx_roundtrip(
        &pptx_copy,
        &pptx_saved,
        "Kar Studio + Kantor Native Slides Verified",
        (4.25, 3.60),
    )
    .expect("pptx roundtrip");
    println!("[PASS PPTX] {} | {}", ppt_report.edited_summary, ppt_report.unedited_check);

    if let Some(root) = std::env::var_os("KAR_ACCEPTANCE_SANDBOX_ROOT") {
        let out_dir = PathBuf::from(root).join("docs/working");
        let _ = std::fs::create_dir_all(&out_dir);
        let _ = std::fs::copy(&docx_saved, out_dir.join("sample_minutes.saved.docx"));
        let _ = std::fs::copy(&xlsx_saved, out_dir.join("sample_formulas.saved.xlsx"));
        let _ = std::fs::copy(&pptx_saved, out_dir.join("sample_deck.saved.pptx"));
    }
}

#[test]
fn coedit_three_scenarios_and_mid_conflict_reconfirm() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (word_demo, excel_demo, ppt_demo) =
        materialize_coedit_demo_files(tmp.path()).expect("materialize demo files");
    assert!(word_demo.is_file() && excel_demo.is_file() && ppt_demo.is_file());

    let mut engine = DemoCoEditEngine::new();
    assert!(engine.is_demo());

    // ── Scenario 1: Word (human edits date, AI edits owner in same paragraph -> both preserved)
    let word_op = engine.proposals()[0].operation_id;
    let word_live_human_date = LiveTargetState {
        scenario: ScenarioKey::WordSameParagraph,
        revision: 2,
        current_value: "发布排期：日期 2026-12-01，负责人：待定，状态：评审中".into(),
        current_aux: String::new(),
        target_exists: true,
        ppt_xy_cm: None,
    };
    let synced_word = engine.sync_live_state(&word_live_human_date).unwrap();
    assert_eq!(synced_word.status, CoEditStatus::ReadyDualKeep);
    assert_eq!(
        synced_word.merged_preview.as_deref(),
        Some("发布排期：日期 2026-12-01，负责人：林远，状态：评审中")
    );
    match engine
        .submit_decision(word_op, UserResolutionChoice::ApplyDualKeep, &word_live_human_date)
        .unwrap()
    {
        SubmissionOutcome::Accepted { proposal, .. } => {
            assert!(
                proposal
                    .resolution_result
                    .as_deref()
                    .unwrap()
                    .contains("2026-12-01")
                    && proposal
                        .resolution_result
                        .as_deref()
                        .unwrap()
                        .contains("林远")
            );
        }
        other => panic!("expected Accepted for Word dual keep, got {other:?}"),
    }

    // ── Scenario 2: Excel same-cell conflict (Keep / Adopt / Manual Merge + mid-conflict revision change!)
    let excel_op = engine.proposals()[1].operation_id;
    let excel_live_rev2 = LiveTargetState {
        scenario: ScenarioKey::ExcelSameCellConflict,
        revision: 2,
        current_value: "135000".into(),
        current_aux: "合计 B4 = 215000".into(),
        target_exists: true,
        ppt_xy_cm: None,
    };
    let synced_excel = engine.sync_live_state(&excel_live_rev2).unwrap();
    assert_eq!(synced_excel.status, CoEditStatus::ConflictNeedsChoice);
    assert_eq!(synced_excel.base_value, "120000");
    assert_eq!(synced_excel.current_value, "135000");
    assert_eq!(synced_excel.proposed_value, "148000");

    // Simulate document changing AGAIN during conflict resolution (rev #2 -> rev #3)!
    let excel_live_rev3 = LiveTargetState {
        scenario: ScenarioKey::ExcelSameCellConflict,
        revision: 3,
        current_value: "139000".into(),
        current_aux: "合计 B4 = 219000".into(),
        target_exists: true,
        ppt_xy_cm: None,
    };
    let stale_attempt = engine
        .submit_decision(
            excel_op,
            UserResolutionChoice::ManualMerge {
                merged_value: "142000".into(),
            },
            &excel_live_rev3,
        )
        .unwrap();
    assert_eq!(
        stale_attempt,
        SubmissionOutcome::NeedsReconfirm {
            previous_revision: 2,
            latest_revision: 3,
            latest_value: "139000".into(),
        }
    );

    // User sees rev #3 and clicks re-confirm -> subsequent ManualMerge succeeds!
    engine.acknowledge_latest_revision(excel_op, 3).unwrap();
    match engine
        .submit_decision(
            excel_op,
            UserResolutionChoice::ManualMerge {
                merged_value: "142000".into(),
            },
            &excel_live_rev3,
        )
        .unwrap()
    {
        SubmissionOutcome::Accepted { proposal, .. } => {
            assert!(proposal.resolution_result.unwrap().contains("142000"));
        }
        other => panic!("expected Accepted after reconfirm, got {other:?}"),
    }

    // Verify KeepCurrent & AdoptProposal branches after reset as well
    engine.reset_scenario(ScenarioKey::ExcelSameCellConflict, 1, None);
    let _ = engine.sync_live_state(&excel_live_rev2);
    assert!(matches!(
        engine
            .submit_decision(excel_op, UserResolutionChoice::KeepCurrent, &excel_live_rev2)
            .unwrap(),
        SubmissionOutcome::Accepted { .. }
    ));
    engine.reset_scenario(ScenarioKey::ExcelSameCellConflict, 1, None);
    let _ = engine.sync_live_state(&excel_live_rev2);
    assert!(matches!(
        engine
            .submit_decision(excel_op, UserResolutionChoice::AdoptProposal, &excel_live_rev2)
            .unwrap(),
        SubmissionOutcome::Accepted { .. }
    ));

    // ── Scenario 3A: PPT human moves text box + AI edits text -> both take effect!
    let ppt_op = engine.proposals()[2].operation_id;
    let ppt_moved = LiveTargetState {
        scenario: ScenarioKey::PptMoveAndText,
        revision: 2,
        current_value: "初版标题：Q4 增长复盘".into(),
        current_aux: String::new(),
        target_exists: true,
        ppt_xy_cm: Some((6.50, 4.80)),
    };
    let synced_ppt = engine.sync_live_state(&ppt_moved).unwrap();
    assert_eq!(synced_ppt.status, CoEditStatus::ReadyDualKeep);
    match engine
        .submit_decision(ppt_op, UserResolutionChoice::ApplyDualKeep, &ppt_moved)
        .unwrap()
    {
        SubmissionOutcome::Accepted { proposal, .. } => {
            let res = proposal.resolution_result.unwrap();
            assert!(res.contains("6.50cm") && res.contains("终版标题：Kar 原生双写工作台发布"));
        }
        other => panic!("expected Accepted for PPT orthogonal merge, got {other:?}"),
    }

    // ── Scenario 3B: PPT target text box deleted -> TargetDeletedConflict!
    engine.reset_scenario(ScenarioKey::PptMoveAndText, 1, Some(2));
    let ppt_deleted = LiveTargetState {
        scenario: ScenarioKey::PptMoveAndText,
        revision: 3,
        current_value: String::new(),
        current_aux: String::new(),
        target_exists: false,
        ppt_xy_cm: None,
    };
    let synced_del = engine.sync_live_state(&ppt_deleted).unwrap();
    assert_eq!(synced_del.status, CoEditStatus::TargetDeletedConflict);
    assert!(
        engine
            .submit_decision(ppt_op, UserResolutionChoice::ApplyDualKeep, &ppt_deleted)
            .is_err(),
        "applying text onto a deleted text box must be rejected as target conflict"
    );
    assert!(matches!(
        engine
            .submit_decision(
                ppt_op,
                UserResolutionChoice::DismissDeletedTarget,
                &ppt_deleted
            )
            .unwrap(),
        SubmissionOutcome::Accepted { .. }
    ));
}

#[tokio::test]
async fn real_codex_send_second_client_mid_tool_disconnect_reconnect_and_history_parity() {
    let temp_holder = tempfile::tempdir().expect("sandbox tempdir");
    let root = std::env::var_os("KAR_ACCEPTANCE_SANDBOX_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temp_holder.path().to_path_buf());
    let chat_dir = root.join("chats");
    let ws_dir = root.join("workspace");
    std::fs::create_dir_all(&chat_dir).unwrap();
    std::fs::create_dir_all(&ws_dir).unwrap();

    // Initialize an isolated git repository so Kar's checkpoint file-change tracker captures edits!
    let run_git = |args: &[&str]| {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(&ws_dir)
            .status()
            .expect("run git");
        assert!(st.success(), "git {args:?} failed");
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.email", "kar-test@local"]);
    run_git(&["config", "user.name", "Kar Test"]);
    std::fs::write(ws_dir.join("note.txt"), "line 1\n").unwrap();
    run_git(&["add", "note.txt"]);
    run_git(&["commit", "-q", "--allow-empty", "-m", "baseline"]);

    let store = ChatStore::open(&chat_dir).expect("open isolated ChatStore");
    let mut opts = SessionOptions::new(&ws_dir);
    opts.approvals = ApprovalMode::Host;

    // 1. Create a REAL Kar chat using the host's installed & logged-in `codex` runtime
    let session = store
        .create("codex", None, opts)
        .await
        .expect("create real codex ChatSession");
    let chat_id = session.info().id;
    println!("[REAL KAR] Created codex chat {chat_id} in isolated store {}", chat_dir.display());

    // 2. Verify single-writer controller lock: a second `KarBridgeHandle` trying to open `chat_id`
    //    automatically enters `LocalReadOnlySubscriber` mode without stealing or breaking the lock!
    let bridge2 = KarBridgeHandle::spawn(chat_dir.clone(), ws_dir.clone());
    bridge2.send(KarCommand::SelectLocalChat {
        chat_id,
        prefer_read_only: false,
    });
    let mut saw_readonly_lock = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(Duration::from_millis(400), bridge2.rx.recv()).await
            && let KarBridgeEvent::AccessModeChanged(ChatAccessMode::LocalReadOnlySubscriber {
                reason,
                ..
            }) = ev
        {
            println!("[SINGLE WRITER VERIFIED] Second local client got read-only subscriber: {reason}");
            saw_readonly_lock = true;
            break;
        }
    }
    assert!(
        saw_readonly_lock,
        "second local client must fall back to read-only subscriber while controller #1 holds lease"
    );
    bridge2.send(KarCommand::Shutdown);

    // 3. Start HTTP/SSE server #1 (`kar::chat::event_router`) on a dedicated localhost port
    let listener1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener1.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let (stop1_tx, stop1_rx) = oneshot::channel::<()>();
    let router1 = event_router(store.clone(), chat_id);
    let srv1 = tokio::spawn(async move {
        let _ = axum::serve(listener1, router1)
            .with_graceful_shutdown(async move {
                let _ = stop1_rx.await;
            })
            .await;
    });

    // 4. Spawn Second Client (`HttpChatFeed` + `ChatReplica`) on its own thread (`GET /snapshot` -> `GET /events`),
    //    and also verify our UI `spawn_http_subscriber` reports `HttpReadOnly` + `Reconnecting` (never demo!) on drop.
    let (ui_evt_tx, ui_evt_rx) = async_channel::unbounded();
    let ui_sub_cancel = kar_desktop::kar_bridge::spawn_http_subscriber(base_url.clone(), ui_evt_tx);

    // Wait until UI subscriber confirms initial `HttpReadOnly` + `Connected`
    let mut ui_connected = false;
    while !ui_connected {
        if let Ok(ev) = ui_evt_rx.recv().await
            && matches!(
                ev,
                KarBridgeEvent::ConnectionStateChanged(FeedConnectionState::Connected { .. })
            )
        {
            ui_connected = true;
        }
    }

    // Dedicated second-client verifier using `HttpChatFeed` + `ChatReplica` directly, signaling when
    // a live tool call (`Item::Tool`) is observed mid-turn so we can sever the HTTP server mid-tool!
    let (tool_seen_tx, tool_seen_rx) = oneshot::channel::<ChatCursor>();
    let (done_tx, done_rx) = oneshot::channel::<(Vec<kar::chat::ChatTurn>, u64, u64)>();
    let sub_url = base_url.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async move {
            let mut feed = HttpChatFeed::new(&sub_url).unwrap();
            let snap = feed.snapshot().await.unwrap();
            let mut info = Some(snap.history.chat.clone());
            let mut replica = ChatReplica::from_snapshot(snap);
            let mut duplicates_ignored = 0u64;
            let mut gaps_recovered = 0u64;
            let mut tool_signaled = Some(tool_seen_tx);
            let mut tested_dup_once = false;

            loop {
                let after = replica.cursor();
                let ev = feed.recv(after).await;
                let maybe_dup = (!tested_dup_once)
                    .then(|| ev.as_ref().ok().cloned())
                    .flatten();
                apply_http_event_with_gap_recovery(
                    &feed,
                    &mut replica,
                    &mut info,
                    ev,
                    &mut duplicates_ignored,
                    &mut gaps_recovered,
                )
                .await
                .unwrap();

                // Verify duplicate event handling once we have an applied event!
                if let Some(dup) = maybe_dup {
                    tested_dup_once = true;
                    let changed = apply_http_event_with_gap_recovery(
                        &feed,
                        &mut replica,
                        &mut info,
                        Ok(dup),
                        &mut duplicates_ignored,
                        &mut gaps_recovered,
                    )
                    .await
                    .unwrap();
                    assert!(!changed, "duplicate ChatEvent must be ignored");
                    assert_eq!(duplicates_ignored, 1);
                }

                // Signal as soon as tool execution starts (or running turn has tool item)
                let has_tool = replica.turns().iter().any(|t| {
                    t.items
                        .iter()
                        .any(|i| matches!(&i.item, Item::Tool { .. }))
                });
                if has_tool && let Some(tx) = tool_signaled.take() {
                    let _ = tx.send(replica.cursor());
                }

                // Finish once the turn reaches Completed AND checkpoint is captured!
                if let Some(t) = replica.turns().first()
                    && t.status == TurnStatus::Completed
                    && t.checkpoint.is_some()
                {
                    // Also test cursor-gap recovery (`InvalidCursor`) before returning!
                    let bogus_gap = ChatEvent {
                        previous: format!("{}:99999999", chat_id).parse().unwrap(),
                        cursor: format!("{}:999999999", chat_id).parse().unwrap(),
                        update: ChatUpdate::NotificationsInterrupted,
                    };
                    let resynced = apply_http_event_with_gap_recovery(
                        &feed,
                        &mut replica,
                        &mut info,
                        Ok(bogus_gap),
                        &mut duplicates_ignored,
                        &mut gaps_recovered,
                    )
                    .await
                    .unwrap();
                    assert!(resynced);
                    assert_eq!(gaps_recovered, 1);

                    let _ = done_tx.send((
                        replica.turns().to_vec(),
                        duplicates_ignored,
                        gaps_recovered,
                    ));
                    break;
                }
            }
        }));
    });

    // 5. Send a REAL prompt via `ChatSession::send(request_id, prompt)` that runs a shell command
    //    sleeping 1.5s + modifying `note.txt`, so we reliably catch it mid-tool-execution!
    let request_id = Uuid::new_v4();
    let prompt = "Use a single shell command `sleep 1.5 && printf 'line 2 from kar\\n' >> note.txt` in the current directory, then reply with `DONE_KAR_CHECK`.";
    let session_arc = std::sync::Arc::new(session);
    let approve_session = session_arc.clone();
    let (stop_approver_tx, mut stop_approver_rx) = tokio::sync::watch::channel(false);
    let approver_task = tokio::spawn(async move {
        let mut answered = std::collections::HashSet::new();
        loop {
            tokio::select! {
                _ = stop_approver_rx.changed() => break,
                _ = tokio::time::sleep(Duration::from_millis(80)) => {
                    if let Ok(h) = approve_session.history() {
                        for req in h.pending_requests {
                            if answered.insert((req.cursor.stream_id, req.cursor.after_seq)) {
                                let decision = req
                                    .controls()
                                    .decisions
                                    .first()
                                    .copied()
                                    .unwrap_or(kar::PermissionDecision::AllowOnce);
                                let _ = approve_session.decide(&req, decision).await;
                            }
                        }
                    }
                }
            }
        }
    });
    let send_session = session_arc.clone();
    let send_task = tokio::spawn(async move { send_session.send(request_id, prompt).await });

    // 6. Wait for Client #2 to observe tool activity in flight, then KILL HTTP Server #1!
    let mid_tool_cursor = tokio::time::timeout(Duration::from_secs(45), tool_seen_rx)
        .await
        .expect("timed out waiting for Codex tool execution")
        .expect("tool_seen_rx closed");
    println!(
        "[MID-TOOL DISCONNECT] Saw live tool execution at cursor {mid_tool_cursor}; stopping HTTP server #1!"
    );
    let _ = stop1_tx.send(());
    srv1.abort();
    let _ = srv1.await;

    // Verify our UI `spawn_http_subscriber` reports `Reconnecting` while offline and NEVER switches to demo!
    let mut saw_ui_reconnecting = false;
    let reconn_deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while tokio::time::Instant::now() < reconn_deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(Duration::from_millis(450), ui_evt_rx.recv()).await
        {
            if let KarBridgeEvent::ChatStateUpdated(ref st) = ev {
                assert!(
                    !st.is_demo,
                    "disconnected real backend MUST NOT switch to demo results!"
                );
            }
            if let KarBridgeEvent::ConnectionStateChanged(FeedConnectionState::Reconnecting {
                ref last_cursor,
                ref reason,
                ..
            }) = ev
            {
                println!(
                    "[UI RECONNECTING STATE VERIFIED] cursor={last_cursor:?} reason={reason}"
                );
                saw_ui_reconnecting = true;
                break;
            }
        }
    }
    assert!(
        saw_ui_reconnecting,
        "UI subscriber must enter Reconnecting state when HTTP server drops mid-tool"
    );

    // 7. Bring HTTP Server #2 back up on the SAME `addr` so Client #2 resumes from `mid_tool_cursor`!
    let listener2 = tokio::net::TcpListener::bind(addr).await.unwrap();
    let (stop2_tx, stop2_rx) = oneshot::channel::<()>();
    let router2 = event_router(store.clone(), chat_id);
    let srv2 = tokio::spawn(async move {
        let _ = axum::serve(listener2, router2)
            .with_graceful_shutdown(async move {
                let _ = stop2_rx.await;
            })
            .await;
    });

    // Wait for `ChatSession::send` AND Client #2's reconnected `ChatReplica` to finish!
    let sent_turn = tokio::time::timeout(Duration::from_secs(90), send_task)
        .await
        .expect("send_task timed out")
        .unwrap()
        .expect("ChatSession::send succeeded");
    assert_eq!(sent_turn.id, request_id);
    assert_eq!(sent_turn.status, TurnStatus::Completed);

    let (replica_turns, dups, gaps) = tokio::time::timeout(Duration::from_secs(20), done_rx)
        .await
        .expect("Client #2 did not catch up after reconnect")
        .unwrap();
    println!(
        "[RECONNECTED & CAUGHT UP] turns={} duplicates_ignored={dups} gaps_recovered={gaps}",
        replica_turns.len()
    );

    // 8. Verify idempotent retry with the EXACT SAME `request_id` UUID!
    let retried_turn = session_arc
        .send(request_id, prompt)
        .await
        .expect("retry with identical request UUID must return existing recorded turn");
    assert_eq!(retried_turn.id, request_id);

    // Close controller cleanly so durable `.karchat` journal settles
    let _ = stop_approver_tx.send(true);
    let _ = approver_task.await;
    let _ = ui_sub_cancel.send(true);
    let _ = stop2_tx.send(());
    let _ = srv2.await;
    if let Ok(owned) = std::sync::Arc::try_unwrap(session_arc) {
        owned.close().await.expect("close ChatSession");
    }

    // 9. Assert exact parity between Client #2's `ChatReplica` and `ChatStore::history`!
    let persisted = store.history(chat_id).expect("read persisted ChatHistory");
    assert_eq!(
        persisted.turns.len(),
        1,
        "idempotent retry must not duplicate turn"
    );
    assert_eq!(
        replica_turns, persisted.turns,
        "Client #2 ChatReplica turns (messages, tool cards, turn status, checkpoints) must match persisted Kar ChatHistory with zero duplicates and zero missing items"
    );
    let final_turn = &persisted.turns[0];
    assert_eq!(final_turn.status, TurnStatus::Completed);
    let tool_cards: Vec<_> = final_turn
        .items
        .iter()
        .filter_map(|i| match &i.item {
            Item::Tool { id, activity } => Some((id, activity)),
            _ => None,
        })
        .collect();
    assert!(
        !tool_cards.is_empty(),
        "persisted turn must contain real tool card(s)"
    );
    assert!(
        tool_cards.iter().all(|(_, a)| a.finished),
        "all tool cards must be updated in place to finished=true"
    );
    let cp_files = &final_turn
        .checkpoint
        .as_ref()
        .expect("recorded git checkpoint")
        .files;
    assert!(
        cp_files.iter().any(|f| f.path == std::path::Path::new("note.txt")),
        "checkpoint must record modified note.txt, got {cp_files:?}"
    );
    println!(
        "[FULL PARITY VERIFIED] 1 turn, {} items, {} tool card(s), checkpoint files: {:?}",
        final_turn.items.len(),
        tool_cards.len(),
        cp_files
    );

    // Copy this real `.karchat` into `/Users/leon/Documents/KWrok/sandbox/chats` if set via env
    // so our live GUI screenshot session can display this exact real Codex chat too!
    if let Some(export_dir) = std::env::var_os("KAR_EXPORT_VERIFIED_CHAT_DIR") {
        let dest = PathBuf::from(export_dir);
        let _ = std::fs::create_dir_all(&dest);
        for entry in std::fs::read_dir(&chat_dir).unwrap().flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().ends_with(".karchat")
                || name.to_string_lossy().ends_with(".lock")
            {
                let _ = std::fs::copy(entry.path(), dest.join(name));
            }
        }
    }
}
