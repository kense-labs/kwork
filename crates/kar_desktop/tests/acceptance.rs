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
async fn office_mcp_server_binds_loopback_and_emits_hosted_http_entry() {
    let (evt_tx, _evt_rx) = async_channel::unbounded();
    let served = kar_desktop::mcp_office::start_office_mcp_server(evt_tx)
        .await
        .expect("bind loopback Office MCP server via kar::serve_mcp_tools");
    let (name, server) = served.entry();
    assert_eq!(name, "kantor");
    match &server {
        kar::McpServer::HostedHttp { url, headers } => {
            assert!(
                url.starts_with("http://127.0.0.1:"),
                "expected loopback MCP URL, got {url}"
            );
            assert!(
                headers.contains_key("Authorization"),
                "expected bearer token header on HostedHttp MCP entry"
            );
        }
        other => panic!("expected HostedHttp server, got {other:?}"),
    }
    served.close().await;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn towork_inspired_remote_pairing_hmac_idempotency_and_revocation_e2e() {
    use kar_desktop::remote_link::{
        PeerRole, RemoteAction, RemoteCommandEnvelope, RemoteLinkCore, RemoteOutcome,
        client_fetch_remote_state, client_redeem_pair_code, client_send_remote_command,
        now_epoch_ms, remote_link_router,
    };
    use std::sync::{Arc, Mutex};

    let (dispatch_tx, dispatch_rx) = async_channel::bounded(16);
    let link = Arc::new(Mutex::new(RemoteLinkCore::new(
        "TestMacHost",
        Some(dispatch_tx),
    )));
    let ticket = {
        let mut guard = link.lock().unwrap();
        guard.issue_pairing_ticket(now_epoch_ms())
    };
    assert_eq!(ticket.code.len(), 6);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let router = remote_link_router(link.clone());
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let srv = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stop_rx.await;
            })
            .await;
    });

    // 1. Redeem 6-digit single-use pairing code over real HTTP `/v1/pair`
    let u = base_url.clone();
    let code = ticket.code.clone();
    let pair_resp = tokio::task::spawn_blocking(move || {
        client_redeem_pair_code(&u, &code, "Leon iPad Controller", PeerRole::Controller)
    })
    .await
    .unwrap()
    .expect("pair code must succeed");
    assert_eq!(pair_resp.host_name, "TestMacHost");
    assert_eq!(pair_resp.role, PeerRole::Controller);

    // 2. Single-use guarantee: replaying the same 6-digit code MUST fail immediately!
    let u2 = base_url.clone();
    let code2 = ticket.code.clone();
    let replay_pair = tokio::task::spawn_blocking(move || {
        client_redeem_pair_code(&u2, &code2, "Attacker", PeerRole::Controller)
    })
    .await
    .unwrap();
    assert!(replay_pair.is_err(), "single-use 6-digit code must be burned");

    // 3. Authenticated `/v1/command` with idempotent `request_id` replay!
    let req_id = Uuid::new_v4();
    let env = RemoteCommandEnvelope {
        request_id: req_id,
        expected_revision: None,
        action: RemoteAction::SendPrompt {
            text: "请汇总第三季度营收并生成 KUI 简报".into(),
            target: None,
        },
    };
    let u3 = base_url.clone();
    let tok3 = pair_resp.token.clone();
    let env3 = env.clone();
    let receipt1 = tokio::task::spawn_blocking(move || {
        client_send_remote_command(&u3, &tok3, &env3)
    })
    .await
    .unwrap()
    .expect("first command");
    assert_eq!(receipt1.outcome, RemoteOutcome::Admitted);

    let (dispatched_peer, dispatched_env) = dispatch_rx.recv().await.expect("forwarded command");
    assert_eq!(dispatched_peer, pair_resp.peer_id);
    assert_eq!(dispatched_env.request_id, req_id);

    // Replay with the EXACT same `request_id` -> returns cached receipt without re-dispatching!
    let u4 = base_url.clone();
    let tok4 = pair_resp.token.clone();
    let receipt2 = tokio::task::spawn_blocking(move || {
        client_send_remote_command(&u4, &tok4, &env)
    })
    .await
    .unwrap()
    .expect("idempotent replay");
    assert_eq!(receipt1, receipt2);
    assert!(
        dispatch_rx.is_empty(),
        "idempotent replay must not re-run business side effects"
    );

    // 4. Instant per-device revocation -> `/v1/state` immediately rejects with 403!
    {
        let mut guard = link.lock().unwrap();
        assert!(guard.revoke_peer(pair_resp.peer_id));
    }
    let u5 = base_url.clone();
    let tok5 = pair_resp.token.clone();
    let after_revoke =
        tokio::task::spawn_blocking(move || client_fetch_remote_state(&u5, &tok5))
            .await
            .unwrap();
    assert!(
        after_revoke.is_err() && after_revoke.unwrap_err().to_string().contains("403"),
        "revoked token must be rejected with HTTP 403"
    );

    let _ = stop_tx.send(());
    let _ = srv.await;
}

#[test]
fn towork_inspired_fence_briefs_schedules_and_budgets_e2e() {
    use kar_desktop::{
        brief::{BriefFormDraft, BriefInventory, build_showcase_kui_brief},
        client_store::{ClientReducer, DetailKey, STREAM_MAX_BYTES, SessionAssociation},
        coedit::{FenceAdmission, FenceOutcome, RevisionFenceLedger},
        schedules::{
            ScheduleCadence, ScheduleRegistry, ScheduleRunOutcome, ScheduleToneChoice,
            ScheduledTask,
        },
        types::OfficeKind,
    };

    let tmp = tempfile::tempdir().expect("tempdir");

    // 1. Revision Fence + idempotent `operation_id` + honest `OutcomeUnknown`
    let mut fence = RevisionFenceLedger::load(tmp.path());
    let op1 = Uuid::new_v4();
    match fence.check_admission(
        "budget.xlsx",
        OfficeKind::Excel,
        Some(op1),
        Some(1),
        "B3".into(),
        "120000".into(),
        "135000".into(),
        None,
        1_000,
    ) {
        FenceAdmission::Proceed { base_revision, .. } => assert_eq!(base_revision, 1),
        other => panic!("expected Proceed, got {other:?}"),
    }
    let rev2 = fence.commit_applied("budget.xlsx", OfficeKind::Excel, op1, 1, "135000", 1_010);
    assert_eq!(rev2, 2);

    // Stale `expected_revision = 1` blocked & queued as conflict (zero silent overwrite!)
    match fence.check_admission(
        "budget.xlsx",
        OfficeKind::Excel,
        Some(Uuid::new_v4()),
        Some(1),
        "B3".into(),
        "135000".into(),
        "999999".into(),
        None,
        1_020,
    ) {
        FenceAdmission::BlockedConflict(c) => {
            assert_eq!(c.expected_revision, 1);
            assert_eq!(c.live_revision, 2);
        }
        other => panic!("expected BlockedConflict, got {other:?}"),
    }

    // Honest `OutcomeUnknown` receipt preserved on disconnect
    let unk_op = Uuid::new_v4();
    let rec = fence.record_outcome_unknown(
        "budget.xlsx",
        OfficeKind::Excel,
        unk_op,
        "IPC disconnected mid-write",
        1_030,
    );
    assert!(matches!(rec.outcome, FenceOutcome::OutcomeUnknown { .. }));

    // 2. Interactive Brief revision fencing + revision-pinned ReviewForm draft
    let chat_id = Uuid::new_v4();
    let mut inv = BriefInventory::default();
    let showcase = build_showcase_kui_brief(None);
    let b1 = inv
        .write_brief(
            Some("q3-brief".into()),
            None,
            Some(chat_id),
            None,
            showcase.current().title.clone(),
            showcase.current().summary.clone(),
            showcase.current().badge.clone(),
            showcase.current().blocks.clone(),
            2_000,
        )
        .unwrap()
        .clone();
    assert_eq!(b1.latest_revision, 1);
    assert!(
        inv.write_brief(
            Some("q3-brief".into()),
            Some(99),
            Some(chat_id),
            None,
            "Stale".into(),
            String::new(),
            None,
            showcase.current().blocks.clone(),
            2_100,
        )
        .is_err(),
        "stale expected_revision on brief must fail"
    );
    let mut draft = BriefFormDraft::default();
    draft.touch_revision(1);
    let msg = draft.format_reply_message(&b1);
    assert!(msg.contains("v1"));
    assert_eq!(draft.has_stale_pin(2), Some(1));
    inv.save(tmp.path(), &chat_id.to_string()).unwrap();
    assert_eq!(
        BriefInventory::load(tmp.path(), &chat_id.to_string())
            .briefs
            .len(),
        1
    );

    // 3. Scheduled Tasks + SessionAssociation + restart `OutcomeUnknown` recovery
    let mut sched = ScheduleRegistry::load_or_seed(tmp.path(), "codex", 5_000);
    let created = sched
        .save_task(
            ScheduledTask {
                id: String::new(),
                revision: 1,
                title: "每日早报".into(),
                prompt: "汇总进度".into(),
                cadence: ScheduleCadence::EveryMinutes(15),
                enabled: true,
                runtime_id: "codex".into(),
                model: None,
                effort: None,
                office_template: None,
                tone: ScheduleToneChoice::Teal,
                next_due_ms: None,
                history: Vec::new(),
            },
            None,
            5_000,
        )
        .unwrap()
        .clone();
    let due = sched.claim_due_runs(5_000 + 16 * 60_000);
    assert!(due.iter().any(|(t, _)| t.id == created.id));
    sched.save(tmp.path()).unwrap();

    // Simulate process restart while run was `Admitted` -> recovers as `OutcomeUnknown`!
    let reloaded = ScheduleRegistry::load_or_seed(tmp.path(), "codex", 5_000 + 17 * 60_000);
    let task_after = reloaded.tasks.iter().find(|t| t.id == created.id).unwrap();
    assert_eq!(
        task_after.history.last().unwrap().outcome,
        ScheduleRunOutcome::OutcomeUnknown
    );

    // 4. ClientReducer `SessionAssociation` + `4 MiB` StreamOutputBudget + `64 MiB` DetailLruCache
    let mut reducer = ClientReducer::new(tmp.path());
    reducer.bind_association(chat_id, SessionAssociation::schedule(&created.id, "每日早报"));
    assert_eq!(
        reducer
            .associations
            .list_associated_chats("schedules", "task", Some(&created.id)),
        vec![chat_id]
    );
    let huge = vec![b'X'; 6 * 1024 * 1024];
    reducer.stream_budget.append("call-big", &huge, true);
    assert!(reducer.stream_budget.used_bytes() <= STREAM_MAX_BYTES);
    let rendered = reducer.stream_budget.read("call-big").unwrap();
    assert!(rendered.contains("4 MiB 内存预算"));

    let protected_key = DetailKey {
        chat_id,
        turn_id: Uuid::new_v4(),
        item_idx: 0,
    };
    let normal_key = DetailKey {
        chat_id: Uuid::new_v4(),
        turn_id: Uuid::new_v4(),
        item_idx: 1,
    };
    reducer
        .detail_cache
        .put(protected_key, "approval payload".into(), true);
    reducer
        .detail_cache
        .put(normal_key, "collapsed tool output".into(), false);
    reducer.detail_cache.release_all_unlocked();
    assert!(reducer.detail_cache.get(&protected_key).is_some());
    assert!(reducer.detail_cache.get(&normal_key).is_none());
}

#[tokio::test]
async fn kui_studio_git_inventory_search_usage_and_handoff_e2e() {
    use kar_desktop::{
        brief::{BriefBlock, build_showcase_kui_brief, parse_mcp_write_brief},
        studio_hub::{
            GlobalSearchHit, build_portable_handoff_context, compute_usage_analytics,
            discover_git_workspace, probe_inventory_with_office, read_workspace_file_preview,
            run_global_search, scan_workspace_tree,
        },
    };
    use std::path::Path;

    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_dir = tmp.path().join("studio_repo");
    std::fs::create_dir_all(ws_dir.join("src")).unwrap();
    std::fs::create_dir_all(ws_dir.join(".agents/skills/office-audit")).unwrap();

    // 1. Initialize real Git repo and test `kar::workbench_git::Repository` + BranchGraph + PatchReview hunks
    let run_git = |args: &[&str]| {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(&ws_dir)
            .status()
            .expect("run git");
        assert!(st.success(), "git {args:?} failed");
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.email", "studio@kar.local"]);
    run_git(&["config", "user.name", "Kar Studio"]);
    std::fs::write(
        ws_dir.join("src/engine.rs"),
        "pub fn run() -> u32 {\n    let version = 1;\n    version\n}\n",
    )
    .unwrap();
    run_git(&["add", "src/engine.rs"]);
    run_git(&["commit", "-q", "-m", "feat: initial studio engine"]);

    run_git(&["checkout", "-q", "-b", "feat/kui-studio"]);
    std::fs::write(
        ws_dir.join("src/engine.rs"),
        "pub fn run() -> u32 {\n    let version = 2;\n    // UNIQUE_STUDIO_NEEDLE_42\n    version * 10\n}\n",
    )
    .unwrap();
    std::fs::write(
        ws_dir.join("src/new_widget.rs"),
        "pub struct StudioWidget;\n",
    )
    .unwrap();

    let git_snap = discover_git_workspace(&ws_dir);
    assert!(git_snap.is_repo);
    assert_eq!(git_snap.current_branch.as_deref(), Some("feat/kui-studio"));
    assert!(git_snap.local_branches.iter().any(|b| b == "feat/kui-studio"));
    assert!(git_snap.dirty_count() >= 2);
    assert!(!git_snap.commits.is_empty());

    let preview = read_workspace_file_preview(&ws_dir, Path::new("src/engine.rs"))
        .expect("read preview with git diff hunks");
    assert_eq!(preview.language, "Rust");
    assert_eq!(preview.total_lines, 5);
    assert!(
        !preview.diff_hunks.is_empty(),
        "modified file should produce unified diff hunks for KUI PatchReview"
    );

    let tree = scan_workspace_tree(&ws_dir, 128);
    assert!(
        tree.iter().any(|item| {
            item.rel_path == Path::new("src")
                && item
                    .children
                    .iter()
                    .any(|c| c.rel_path == Path::new("src/engine.rs"))
        }),
        "workspace tree should include src/engine.rs under src/"
    );

    // 2. Workspace Skills (`SKILL.md`), Slash Commands, MCP & Tool Schemas
    std::fs::write(
        ws_dir.join(".agents/skills/office-audit/SKILL.md"),
        "---\nname: office-audit\ndescription: 跨文档核验预算表格公式与纪要数字一致性\n---\n# Office Audit\nCheck formulas.\n",
    )
    .unwrap();
    let inv = probe_inventory_with_office(&ws_dir, "codex", true).await;
    assert!(
        inv.skills.iter().any(|s| s.name == "office-audit"),
        "expected local SKILL.md discovery"
    );
    let office = inv.mcp_servers.iter().find(|server| server.id == "kantor").unwrap();
    assert_eq!(office.tool_count, inv.tools.iter().filter(|t| t.server_id == office.id).count());
    let edit = inv.tools.iter().find(|tool| tool.name == "office_edit_word").unwrap();
    assert!(edit.schema_json["properties"]["tab_id"].is_object());
    assert!(!inv.tools.iter().any(|tool| tool.name == "word_replace_paragraph"));

    // 3. Global Search 2.0 (file path + line content hits)
    let path_hits = run_global_search(&ws_dir, None, "new_widget", 8);
    assert!(
        path_hits
            .iter()
            .any(|h| matches!(h, GlobalSearchHit::FilePath { rel_path } if rel_path.ends_with("new_widget.rs")))
    );
    let content_hits = run_global_search(&ws_dir, None, "UNIQUE_STUDIO_NEEDLE_42", 8);
    assert!(
        content_hits
            .iter()
            .any(|h| matches!(h, GlobalSearchHit::FileContent { line: 3, .. }))
    );

    // 4. Usage & Cache Analytics + Cross-Runtime Session Handoff Context
    let usage = compute_usage_analytics(None, &[], &[]);
    assert_eq!(usage.total_input_tokens, 0);
    assert!(!usage.has_usage);
    assert_eq!(usage.daily_activity.len(), 28);
    assert!(usage.by_runtime.is_empty());
    assert!(usage.daily_activity.iter().all(|(_, turns)| *turns == 0.));
    let reported = kar::UsageSnapshot {
        cursor: kar::Cursor { stream_id: Uuid::new_v4(), after_seq: 1 },
        usage: kar::SessionUsage {
            total: Some(kar::TokenTotals {
                input: 100, output: 15, cache_read: Some(0), ..Default::default()
            }),
            ..Default::default()
        },
        children: Default::default(),
    };
    let usage = compute_usage_analytics(None, &[reported], &[]);
    assert!(usage.has_usage && usage.has_cache_read);
    assert!(!usage.has_cache_write && !usage.has_reasoning);
    assert_eq!((usage.total_input_tokens, usage.total_output_tokens), (100, 15));
    assert_eq!(usage.total_cached_tokens, 0);


    let relay = build_portable_handoff_context(
        "codex",
        "claude",
        &[("核对 Q4 预算".into(), "已完成 B4 公式修复".into())],
        "请继续输出 PPT 汇报页",
        2048,
    );
    assert!(relay.contains("Claude Code"));
    assert!(relay.contains("B4 公式修复"));

    // 5. Expanded 14-Block KarBrief (`RadarChart`, `Gauge`, `LineChart`, `FunnelChart`, `JsonTree`)
    let showcase = build_showcase_kui_brief(None);
    let rev_v1 = showcase.at_revision(Some(1)).expect("brief v1");
    assert!(
        rev_v1
            .blocks
            .iter()
            .any(|b| matches!(b, BriefBlock::RadarChart { .. }))
    );
    assert!(
        rev_v1
            .blocks
            .iter()
            .any(|b| matches!(b, BriefBlock::Gauge { .. }))
    );

    let raw_args = serde_json::json!({
        "title": "扩展可视化简报验证",
        "summary": "验证折线图、漏斗图与 JSON 树组件解析",
        "blocks": [
            {
                "type": "line_chart",
                "title": "7 日吞吐趋势",
                "samples": [
                    {"label": "D1", "value": 120.0},
                    {"label": "D2", "value": 168.0},
                    {"label": "D3", "value": 215.0}
                ]
            },
            {
                "type": "funnel_chart",
                "title": "自动化采纳漏斗",
                "segments": ["流转量"],
                "stages": [
                    {"id": "s1", "label": "提案生成", "values": [100.0]},
                    {"id": "s2", "label": "人工审阅", "values": [84.0]},
                    {"id": "s3", "label": "合入主干", "values": [76.0]}
                ]
            },
            {
                "type": "json_tree",
                "title": "审计载荷",
                "data": {"ok": true, "items": [1, 2, 3]}
            }
        ]
    });
    let parsed = parse_mcp_write_brief(raw_args.as_object().unwrap())
        .expect("parse extended brief blocks");
    assert_eq!(parsed.5.len(), 3);
    assert!(matches!(parsed.5[0], BriefBlock::LineChart { .. }));
    assert!(matches!(parsed.5[1], BriefBlock::FunnelChart { .. }));
    assert!(matches!(parsed.5[2], BriefBlock::JsonTree { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn decluttered_studio_worktree_unstage_terminal_and_code_selection_e2e() {
    use kar_desktop::{
        studio_hub::{
            ProjectRegistry, create_isolated_worktree_for_chat, discover_git_workspace,
            read_single_file_diff_hunks, scan_workspace_tree_with_options,
            switch_or_create_git_branch, unstage_git_paths,
        },
        types::{OfficeKind, SelectionLocator, SelectionTarget, strip_selection_envelope},
    };
    use std::path::Path;

    let tmp = tempfile::tempdir().expect("tempdir");
    let store_dir = tmp.path().join("chat_store");
    let ws_dir = tmp.path().join("repo_main");
    std::fs::create_dir_all(ws_dir.join("src")).unwrap();
    std::fs::create_dir_all(ws_dir.join(".hidden_cache")).unwrap();
    std::fs::write(ws_dir.join(".hidden_cache/secret.tmp"), "ignore\n").unwrap();

    let run_git = |args: &[&str]| {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(&ws_dir)
            .status()
            .expect("run git");
        assert!(st.success(), "git {args:?} failed");
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.email", "studio@kar.local"]);
    run_git(&["config", "user.name", "Kar Studio"]);
    std::fs::write(ws_dir.join("src/lib.rs"), "pub fn answer() -> u32 {\n    41\n}\n").unwrap();
    run_git(&["add", "src/lib.rs"]);
    assert!(!read_single_file_diff_hunks(&ws_dir, Path::new("src/lib.rs"), false).is_empty());
    run_git(&["commit", "-q", "-m", "feat: baseline"]);
    assert!(read_single_file_diff_hunks(&ws_dir, Path::new("src/lib.rs"), false).is_empty());

    // 1. Default workspace tree hides dot-folders (`.hidden_cache`), keeping `src` first!
    let clean_tree = scan_workspace_tree_with_options(&ws_dir, 64, false);
    assert!(clean_tree.iter().any(|n| n.rel_path == Path::new("src")));
    assert!(
        !clean_tree
            .iter()
            .any(|n| n.rel_path == Path::new(".hidden_cache")),
        "dot-folders must be hidden by default"
    );
    let hidden_tree = scan_workspace_tree_with_options(&ws_dir, 64, true);
    assert!(
        hidden_tree
            .iter()
            .any(|n| n.rel_path == Path::new(".hidden_cache")),
        "show_hidden=true must reveal dot-folders"
    );

    // 2. Stage -> Unstage (`git reset HEAD --`) + Inline Diff Hunks + Branch Switcher!
    std::fs::write(ws_dir.join("src/lib.rs"), "pub fn answer() -> u32 {\n    42\n}\n").unwrap();
    run_git(&["add", "src/lib.rs"]);
    assert_eq!(discover_git_workspace(&ws_dir).staged_count(), 1);

    let hunks = read_single_file_diff_hunks(&ws_dir, Path::new("src/lib.rs"), false);
    assert!(!hunks.is_empty(), "staged file diff must yield inline hunks");

    unstage_git_paths(&ws_dir, &[PathBuf::from("src/lib.rs")]).expect("unstage path");
    let snap_after_unstage = discover_git_workspace(&ws_dir);
    assert_eq!(
        snap_after_unstage.staged_count(),
        0,
        "unstage must move file back out of index"
    );
    assert!(snap_after_unstage.dirty_count() >= 1);

    // Commit cleanly so we can test branch switch & isolated worktree creation
    run_git(&["add", "-A"]);
    run_git(&["commit", "-q", "-m", "fix: answer 42"]);

    let switched =
        switch_or_create_git_branch(&ws_dir, "feat/clean-ui", true).expect("create branch");
    assert_eq!(switched, "feat/clean-ui");
    assert_eq!(
        discover_git_workspace(&ws_dir).current_branch.as_deref(),
        Some("feat/clean-ui")
    );

    // 3. ProjectRegistry + Real Isolated Git Worktree (`kar::workbench_git::Repository::create_worktree`)
    let mut reg = ProjectRegistry::load_or_seed(&store_dir, &ws_dir);
    let chat_id = Uuid::new_v4();
    assert!(reg.toggle_pin(chat_id));
    let wt_meta =
        create_isolated_worktree_for_chat(&ws_dir, &store_dir, chat_id).expect("create worktree");
    assert!(wt_meta.worktree_path.is_dir());
    assert!(wt_meta.worktree_path.join("src/lib.rs").is_file());
    reg.chat_worktrees.insert(chat_id, wt_meta.clone());
    reg.save(&store_dir).unwrap();
    let reloaded = ProjectRegistry::load_or_seed(&store_dir, &ws_dir);
    assert!(reloaded.is_pinned(chat_id));
    assert_eq!(
        reloaded.worktree_for(chat_id).map(|w| &w.branch),
        Some(&wt_meta.branch)
    );

    // 4. `SelectionLocator::CodeLines` citation envelope & prompt round-trip (Bug #9 fix)
    let code_sel = SelectionTarget {
        tab_id: 0,
        file_path: Some(ws_dir.join("src/lib.rs")),
        doc_name: "src/lib.rs".into(),
        kind: OfficeKind::Word,
        base_revision: None,
        locator: SelectionLocator::CodeLines {
            rel_path: PathBuf::from("src/lib.rs"),
            language: "Rust".into(),
            start_line: 1,
            end_line: 3,
            total_lines: 3,
        },
        excerpt: "pub fn answer() -> u32 {\n    42\n}".into(),
    };
    assert!(code_sel.is_code());
    assert!(code_sel.detail_line().contains("[CODE:Rust]"));
    let composed = code_sel.compose_prompt("请解释这段函数");
    assert!(composed.contains("```rust"));
    assert_eq!(strip_selection_envelope(&composed), "请解释这段函数");

    // 5. Fast startup provider seed (<800ms) + Streaming Integrated Terminal (`RunWorkspaceCommand`)
    let bridge = KarBridgeHandle::spawn(store_dir.clone(), wt_meta.worktree_path.clone());
    let run_id = Uuid::new_v4();
    bridge.send(KarCommand::RunWorkspaceCommand {
        id: run_id,
        cwd: wt_meta.worktree_path.clone(),
        command: "printf 'KAR_TERMINAL_OK\\n'".into(),
    });

    let mut saw_fast_providers = false;
    let mut term_out = String::new();
    let mut term_exit = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_millis(300), bridge.rx.recv()).await
        {
            match ev {
                KarBridgeEvent::ProvidersUpdated(list) if !list.is_empty() => {
                    saw_fast_providers = true;
                }
                KarBridgeEvent::TerminalOutputChunk { id, chunk } if id == run_id => {
                    term_out.push_str(&chunk);
                }
                KarBridgeEvent::TerminalCommandFinished { id, exit_code } if id == run_id => {
                    term_exit = Some(exit_code);
                }
                _ => {}
            }
            if saw_fast_providers && term_exit.is_some() {
                break;
            }
        }
    }
    bridge.send(KarCommand::Shutdown);

    assert!(
        saw_fast_providers,
        "fast_seed_providers_from_path must emit ProvidersUpdated immediately"
    );
    assert!(
        term_out.contains("KAR_TERMINAL_OK"),
        "integrated terminal must stream command output, got: {term_out:?}"
    );
    assert_eq!(term_exit, Some(0));
}
