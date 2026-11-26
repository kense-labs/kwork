//! Optional driver for the real Claude CLI against an isolated local Messages endpoint.
use super::*;
use anyhow::{Context as _, ensure};
use axum::{Json, Router, routing::post};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires KAR_CLAUDE_BIN >= 2.1.326"]
async fn real_side_question_uses_saved_conversation_and_leaves_main_turns_intact()
-> anyhow::Result<()> {
    let program = PathBuf::from(
        std::env::var_os("KAR_CLAUDE_BIN").context("supply the official Claude binary")?,
    )
    .canonicalize()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let workspace = root.join("work");
    let home = root.join("home");
    std::fs::create_dir(&workspace)?;
    std::fs::create_dir(&home)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router=Router::new().route("/v1/messages",post(|Json(body):Json<Value>|async move{
        let text=if body["messages"].to_string().contains("SIDE_FORK_MARKER") {"REAL_CLAUDE_FORK_REPLY"} else {"MAIN_THREAD_REPLY"};
        let frames=[
            json!({"type":"message_start","message":{"id":"msg_fixture","type":"message","role":"assistant","model":body["model"],"content":[],"stop_reason":null,"usage":{"input_tokens":4,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"})];
        let sse=frames.iter().map(|v|format!("event: {}\ndata: {v}\n\n",v["type"].as_str().unwrap())).collect::<String>();
        ([("content-type","text/event-stream")],sse)
    }));
    let model = tokio::spawn(async move { axum::serve(listener, router).await });
    let wrapper = root.join("claude");
    let home = home.to_string_lossy();
    let shell_home = shell_words::quote(&home);
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexport HOME={shell_home} CLAUDE_CONFIG_DIR={shell_home} ANTHROPIC_BASE_URL=http://{address} ANTHROPIC_API_KEY=loopback-only ANTHROPIC_AUTH_TOKEN='' CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1\nexec {} \"$@\"\n",
            shell_words::quote(&program.to_string_lossy())
        ),
    )?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))?;
    let store = ChatStore::open(root.join("chats"))?;
    let mut opts = SessionOptions::new(&workspace);
    opts.model = Some("sonnet".into());
    let session = store.create("claude", Some(&wrapper), opts).await?;
    let chat = session.info().id;
    let native = session.info().native_id.clone();
    let turn = session.send(Uuid::new_v4(), "A source question").await?;
    ensure!(
        turn.status == TurnStatus::Completed,
        "source Claude turn failed: {:?}",
        turn.failure
    );
    session.close().await?;
    let bridge = KarBridgeHandle::spawn(store.directory().into(), workspace);
    bridge.send(KarCommand::SelectLocalChat {
        chat_id: chat,
        prefer_read_only: false,
    });
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            if let KarBridgeEvent::SideQuestionCapability {
                chat: id,
                native: source,
                supported,
            } = bridge.rx.recv().await?
            {
                ensure!(
                    id == chat && source == native && supported,
                    "Claude did not declare side questions"
                );
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await
    .context("no native capability announcement")??;
    let req = Uuid::new_v4();
    bridge.send(KarCommand::AskSideQuestion {
        chat_id: chat,
        request_id: req,
        question: "SIDE_FORK_MARKER".into(),
    });
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if let KarBridgeEvent::SideQuestionAnswered {
                request_id, result, ..
            } = bridge.rx.recv().await?
            {
                if request_id == req {
                    ensure!(result.map_err(anyhow::Error::msg)? == "REAL_CLAUDE_FORK_REPLY");
                    return Ok::<_, anyhow::Error>(());
                }
            }
        }
    })
    .await
    .context("no reply from the real side fork")??;
    let history = store.history(chat)?;
    ensure!(
        history.chat.native_id == native && history.turns.len() == 1,
        "side question changed its source"
    );
    bridge.send(KarCommand::Shutdown);
    model.abort();
    println!(
        "Claude declared the installed capability; Kar opened a disposable native fork, returned its reply and preserved the original native ID and one source turn."
    );
    Ok(())
}
