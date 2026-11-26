//! Run with `cargo run -p kar_desktop --example native_composer_e2e` on a desktop
//! with a logged-in Codex runtime. Optionally pass a parent directory for the evidence.
//! On macOS, run `create-review DIR`, `resume-review DIR`, then `draft-review DIR`
//! with the same new directory to exercise relaunch, file review, follow-up, and drafting.
#[path = "support/review_workflow.rs"]
mod review_workflow;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail, ensure};
use gpui_kit::{prelude::*, *};
use kar::chat::{ChatStore, Item, Role, TurnStatus};
use kar_desktop::{
    init_runtime_ui,
    kar_bridge::KarCommand,
    studio_window_options,
    workbench::{KarWorkbench, WorkbenchConfig},
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if matches!(
        args.first().and_then(|s| s.to_str()),
        Some("create-review" | "resume-review" | "draft-review")
    ) {
        let phase = args[0].to_str().unwrap();
        let root = PathBuf::from(
            args.get(1)
                .context("provide an isolated evidence directory")?,
        );
        return review_workflow::launch(root, phase.to_owned());
    }
    let parent = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = tempfile::Builder::new()
        .prefix("native-composer-")
        .tempdir_in(parent)?
        .keep();
    let config = WorkbenchConfig {
        workspace_dir: root.join("workspace"),
        chat_dir: root.join("chats"),
        docs_dir: root.join("docs"),
        ..Default::default()
    };
    fs::create_dir_all(&config.workspace_dir)?;
    let chat_dir = config.chat_dir.clone();
    println!("Evidence: {}", root.display());
    gpui_kit::application()
        .with_assets(kantor_ui::KantorAssets)
        .run(move |cx| {
            init_runtime_ui(cx);
            let mut options = studio_window_options(
                "Kar · E2E",
                size(px(1200.), px(800.)),
                size(px(920.), px(580.)),
                cx,
            );
            options.focus = false;
            let (window, wb) = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| KarWorkbench::new(config, window, cx))
            })
            .expect("open native Kar window");
            cx.spawn(async move |cx| {
                let result = run_composer_test(window, &wb, &root, &chat_dir, cx).await;
                let _ = window.update(cx, |_, _, cx| wb.read(cx).bridge.send(KarCommand::Shutdown));
                if let Err(err) = result {
                    eprintln!("Native composer E2E failed: {err:#}");
                    std::process::exit(1);
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    Ok(())
}

async fn capture(handle: AnyWindowHandle, cx: &mut AsyncApp, path: &Path) -> Result<()> {
    // Background windows can coalesce all frames of the chat transition into one.
    // Let GPUI animate and resolve SVG resources before reading the Metal surface.
    let until = Instant::now() + Duration::from_millis(650);
    loop {
        handle.update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })?;
        if Instant::now() >= until {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(32))
            .await;
    }
    #[cfg(target_os = "macos")]
    handle.update(cx, |_, window, _| -> Result<()> {
        window.render_to_image()?.save(path)?;
        Ok(())
    })??;
    #[cfg(not(target_os = "macos"))]
    let _ = path;
    Ok(())
}

async fn run_composer_test(
    window: AnyWindowHandle,
    wb: &Entity<KarWorkbench>,
    root: &Path,
    chat_dir: &Path,
    cx: &mut AsyncApp,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let ready = window.update(cx, |_, _, cx| {
            wb.read(cx)
                .providers
                .iter()
                .any(|p| p.id == "codex" && p.is_ready())
        })?;
        if ready {
            break;
        }
        ensure!(Instant::now() < deadline, "Codex did not become ready");
        cx.background_executor()
            .timer(Duration::from_millis(120))
            .await;
    }
    let marker = format!("kar{}", uuid::Uuid::new_v4().simple());
    let prompt = format!("reply with only {marker}");
    window.update(cx, |_, window, cx| -> Result<()> {
        ensure!(
            wb.read(cx).selected_runtime == "codex",
            "unexpected default runtime"
        );
        let shortcut = if cfg!(target_os = "macos") {
            "cmd-n"
        } else {
            "ctrl-n"
        };
        window.dispatch_keystroke(Keystroke::parse(shortcut)?, cx);
        window.refresh();
        window.draw(cx).clear(cx);
        for c in prompt.chars() {
            let key = if c == ' ' {
                "space".into()
            } else {
                c.to_string()
            };
            ensure!(
                window.dispatch_keystroke(
                    Keystroke {
                        key: key.clone(),
                        ..Default::default()
                    },
                    cx
                ),
                "composer rejected {key}"
            );
        }
        Ok(())
    })??;
    capture(window, cx, &root.join("typed.png")).await?;
    window.update(cx, |_, window, cx| -> Result<()> {
        window.dispatch_keystroke(Keystroke::parse("enter")?, cx);
        Ok(())
    })??;
    println!("Entered the prompt with GPUI keystrokes and submitted it with Return.");

    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let complete = window.update(cx, |_, _, cx| -> Result<bool> {
            let state = &wb.read(cx).projected;
            ensure!(!state.is_demo && state.turns.len() <= 1, "unexpected conversation state");
            let Some(turn) = state.turns.first() else { return Ok(false) };
            if turn.status == TurnStatus::Running { return Ok(false); }
            ensure!(turn.status == TurnStatus::Completed, "turn ended with {:?}", turn.status);
            let has_message = |role, expected: &str| turn.items.iter().any(|item| {
                matches!(&item.item, Item::Message { role: r, text, .. } if *r == role && text.trim() == expected)
            });
            ensure!(has_message(Role::User, &prompt), "the stored user message differs from the typed text");
            ensure!(has_message(Role::Assistant, &marker), "reply did not match the unique marker");
            let id = state.chat_id.context("missing chat id")?;
            let history = ChatStore::open(chat_dir)?.history(id)?;
            if history.turns != state.turns { return Ok(false); }
            println!("Verified real response {marker}, one completed turn and matching persisted history in {id}.");
            Ok(true)
        })??;
        if complete {
            capture(window, cx, &root.join("reply.png")).await?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            capture(window, cx, &root.join("timeout.png")).await?;
            bail!("desktop did not receive a completed response within 90s");
        }
        cx.background_executor()
            .timer(Duration::from_millis(150))
            .await;
    }
}
