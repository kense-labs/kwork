//! Exercise both desktop subscribers against the same deterministic Kar journal.
use super::*;
use anyhow::ensure;
use serde_json::{Value, json};
use std::{fs::File, io::Write};

fn append(journal: &mut File, records: &[Value]) -> Result<()> {
    journal.lock()?;
    for record in records {
        writeln!(journal, "{record}")?;
    }
    journal.unlock()?;
    Ok(())
}

async fn wait_for(
    rx: &async_channel::Receiver<KarBridgeEvent>,
    mut accept: impl FnMut(&ProjectedChatState) -> bool,
) -> Result<ProjectedChatState> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let KarBridgeEvent::ChatStateUpdated(state) = rx.recv().await?
                && accept(&state)
            {
                return Ok(state);
            }
        }
    })
    .await?
}

fn reply(state: &ProjectedChatState) -> &str {
    state
        .turns
        .last()
        .and_then(|turn| {
            turn.items.iter().find_map(|row| match &row.item {
                Item::Message {
                    role: Role::Assistant,
                    text,
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn text_bursts_reach_local_and_http_before_completion_then_match_history() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let store = ChatStore::open(dir.path())?;
    let chat = Uuid::new_v4();
    let turn = Uuid::new_v4();
    let stream = Uuid::new_v4();
    let lease = File::create(dir.path().join(format!("{chat}.lock")))?;
    lease.lock()?;
    let mut journal = File::create(dir.path().join(format!("{chat}.karchat")))?;
    append(
        &mut journal,
        &[
            json!({"format":"kar-chat/1","chat":{
                "id":chat,"workspace":dir.path(),"runtime":"codex","protocol":"codex","native_id":"native",
                "created_ms":1,"program":"codex","approvals":"host"
            }}),
            json!({"kind":"opened","stream":stream}),
        ],
    )?;
    let (local_tx, local) = async_channel::unbounded();
    let local_cancel =
        spawn_local_follower(store.clone(), dir.path().to_owned(), chat, None, local_tx);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let router = event_router(store.clone(), chat);
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let (http_tx, http) = async_channel::unbounded();
    let http_cancel = spawn_http_subscriber(url, http_tx);
    for rx in [&local, &http] {
        wait_for(rx, |state| state.chat_id == Some(chat)).await?;
    }

    let fragment = |seq, text: &str, complete| {
        json!({"kind":"frame","id":turn,"frame":{
            "cursor":{"stream_id":stream,"after_seq":seq},"source":{"session_id":"native","path":[]},
            "edits":[{"op":"text","role":"assistant","text":text,"key":"reply","final_text":complete}]
        }})
    };
    let mut first_burst = vec![
        json!({"kind":"begin","id":turn,"at":2,"text":"test"}),
        fragment(1, "首", false),
    ];
    first_burst.extend((2..=96).map(|seq| fragment(seq, "界", false)));
    append(&mut journal, &first_burst)?;
    let prefix = format!("首{}", "界".repeat(95));
    for rx in [&local, &http] {
        let first = wait_for(rx, |state| !reply(state).is_empty()).await?;
        ensure!(
            reply(&first) == "首",
            "first fragment was held until the burst finished"
        );
        let mut updates = 0;
        let flushed = wait_for(rx, |state| {
            updates += 1;
            reply(state) == prefix
        })
        .await?;
        ensure!(
            updates < 95,
            "every buffered fragment triggered a full desktop projection"
        );
        ensure!(
            flushed.turns[0].status == TurnStatus::Running,
            "tail waited for completion"
        );
    }
    // The queue has separate durable storage. A second client can enqueue between text deltas.
    let queued = Uuid::new_v4();
    store.enqueue(chat, queued, "下一步")?;
    append(&mut journal, &[fragment(97, "中", false)])?;
    wait_for(&local, |state| {
        reply(state).ends_with('中') && state.queued_inputs.iter().any(|q| q.id == queued)
    })
    .await?;
    let complete = format!("{prefix}中尾。");
    append(
        &mut journal,
        &[
            fragment(98, "尾", false),
            fragment(99, &complete, true),
            json!({"kind":"end","id":turn,"status":"completed"}),
        ],
    )?;
    let history = store.history(chat)?;
    for rx in [&local, &http] {
        let last = wait_for(rx, |state| {
            state
                .turns
                .last()
                .is_some_and(|t| t.status == TurnStatus::Completed)
        })
        .await?;
        ensure!(
            reply(&last) == complete && last.turns == history.turns,
            "terminal projection lost or duplicated text"
        );
    }
    let _ = local_cancel.send(true);
    let _ = http_cancel.send(true);
    server.abort();
    Ok(())
}

#[test]
fn repeated_tool_snapshots_and_replacements_keep_exact_bytes() {
    let chat = Uuid::new_v4();
    let mut budget = crate::client_store::StreamOutputBudget::default();
    let old = build_demo_seed_turns();
    let mut new = old.clone();
    let index = new[0]
        .items
        .iter()
        .position(|row| matches!(row.item, Item::Tool { .. }))
        .unwrap();
    let key = format!("{chat}/{}/{index}", old[0].id);
    budget.sync_tool_snapshots(chat, &[], &old);
    let bytes = budget.used_bytes();
    assert!(bytes > 0);
    budget.sync_tool_snapshots(chat, &old, &old);
    assert_eq!(budget.used_bytes(), bytes);
    let Item::Tool { activity, .. } = &mut new[0].items[index].item else {
        unreachable!()
    };
    activity.result.as_mut().unwrap().content = Some(vec![
        kar::OutputPart::Text {
            text: "工具".into(),
        },
        kar::OutputPart::Text {
            text: "输出".into(),
        },
    ]);
    budget.sync_tool_snapshots(chat, &old, &new);
    budget.sync_tool_snapshots(chat, &[], &new);
    assert_eq!(budget.used_bytes(), "工具输出".len());
    assert_eq!(budget.read(&key).as_deref(), Some("工具输出"));
}
