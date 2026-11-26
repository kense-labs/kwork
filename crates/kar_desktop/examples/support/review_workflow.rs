//! Two real desktop processes, a persisted Kar session, and pointer hits based on visible text.
use super::*;
use kar_desktop::types::ChatAccessMode;
use std::process::Command;
use uuid::Uuid;

const FILE: &str = "release.txt";
const OTHER: &str = "appendix.txt";

pub(super) fn launch(root: PathBuf, phase: String) -> Result<()> {
    let workspace = root.join("workspace");
    let marker_path = root.join("marker");
    if phase == "create-review" {
        ensure!(
            !root.exists(),
            "provide a new directory for the create phase"
        );
        fs::create_dir_all(&workspace)?;
        fs::write(workspace.join(FILE), "draft\n")?;
        fs::write(workspace.join(OTHER), "extra\n")?;
        // Vision reads this value back from the diff. Spell it with words so a
        // visually ambiguous hex digit (e.g. zero) cannot spoil a correct run.
        let words = [
            "amber", "beech", "cedar", "dawn", "fern", "frost", "haze", "jade",
            "maple", "mint", "moss", "pine", "rain", "sage", "sand", "wave",
        ];
        let nonce = Uuid::new_v4();
        let [a, b, ..] = *nonce.as_bytes();
        let marker = format!(
            "ready-{}-{}",
            words[a as usize % words.len()],
            words[b as usize % words.len()]
        );
        fs::write(&marker_path, &marker)?;
        fs::write(
            workspace.join("update-fixture.sh"),
            format!(
                "set -eu\nprintf '%s\\n' '{marker}' > release.txt\nprintf '%s\\n' appendix-ready > appendix.txt\ncat release.txt\n"
            ),
        )?;
        for args in [
            &["init", "-q"][..],
            &["add", "."],
            &[
                "-c",
                "user.name=Kar Test",
                "-c",
                "user.email=kar@localhost",
                "commit",
                "-qm",
                "Baseline",
            ],
        ] {
            ensure!(
                Command::new("git")
                    .args(args)
                    .current_dir(&workspace)
                    .status()?
                    .success(),
                "git {args:?}"
            );
        }
    } else if phase == "resume-review" {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(async {
            let store = ChatStore::open(root.join("chats"))?;
            let decoy = store
                .create("codex", None, kar::SessionOptions::new(&workspace))
                .await?;
            println!("Newer unrelated chat: {}", decoy.info().id);
            decoy.close().await
        })?;
        fs::write(workspace.join(FILE), "later-worktree-only\n")?;
    }
    let src = root.join("locate_text.swift");
    fs::write(&src, include_str!("locate_text.swift"))?;
    ensure!(
        Command::new("swiftc")
            .arg(src)
            .arg("-o")
            .arg(root.join("locate-text"))
            .status()?
            .success(),
        "compile macOS Vision test locator"
    );
    let marker = fs::read_to_string(marker_path)?;
    let config = WorkbenchConfig {
        workspace_dir: workspace,
        chat_dir: root.join("chats"),
        docs_dir: root.join("docs"),
        ..Default::default()
    };
    gpui_kit::application()
        .with_assets(kantor_ui::KantorAssets)
        .run(move |cx| {
            init_runtime_ui(cx);
            let mut opts = studio_window_options(
                "Kar · Review E2E",
                size(px(1280.), px(850.)),
                size(px(920.), px(580.)),
                cx,
            );
            opts.focus = false;
            let (handle, wb) = gpui_kit::open_window(opts, cx, move |window, cx| {
                cx.new(|cx| KarWorkbench::new(config, window, cx))
            })
            .expect("open workbench");
            cx.spawn(async move |cx| {
                let outcome = run(&phase, &root, &marker, handle, &wb, cx).await;
                if let Err(err) = outcome {
                    let _ = capture(handle, cx, &root.join("failed.png")).await;
                    eprintln!("Review E2E failed: {err:#}");
                    std::process::exit(1);
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    Ok(())
}

async fn wait_for(
    handle: AnyWindowHandle,
    wb: &Entity<KarWorkbench>,
    cx: &mut AsyncApp,
    label: &str,
    mut predicate: impl FnMut(&KarWorkbench) -> bool,
) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(100);
    loop {
        if let Some(reason) = handle.update(cx, |_, _, cx| match &wb.read(cx).access_mode {
            ChatAccessMode::ArchivedReadOnly { reason, .. }
            | ChatAccessMode::LocalReadOnlySubscriber { reason, .. } => Some(reason.clone()),
            _ => None,
        })? {
            bail!("while waiting for {label}: {reason}");
        }
        if handle.update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            predicate(wb.read(cx))
        })? {
            return Ok(());
        }
        ensure!(Instant::now() < until, "timed out waiting for {label}");
        cx.background_executor()
            .timer(Duration::from_millis(150))
            .await;
    }
}

fn type_and_send(handle: AnyWindowHandle, cx: &mut AsyncApp, text: &str) -> Result<()> {
    handle.update(cx, |_, window, cx| -> Result<()> {
        for c in text.chars() {
            let key = if c == ' ' {
                "space".to_owned()
            } else {
                c.to_string()
            };
            ensure!(
                window.dispatch_keystroke(
                    Keystroke {
                        key,
                        ..Default::default()
                    },
                    cx
                ),
                "input rejected {c}"
            );
        }
        window.dispatch_keystroke(Keystroke::parse("enter")?, cx);
        Ok(())
    })?
}

fn locate(root: &Path, image: &Path, text: &str) -> Result<(f32, f32)> {
    let out = Command::new(root.join("locate-text"))
        .arg(image)
        .arg(text)
        .output()?;
    ensure!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = String::from_utf8(out.stdout)?;
    let (x, y) = value.trim().split_once(' ').context("Vision coordinates")?;
    Ok((x.parse()?, y.parse()?))
}

fn click_text(
    root: &Path,
    image: &Path,
    text: &str,
    handle: AnyWindowHandle,
    cx: &mut AsyncApp,
) -> Result<()> {
    let (x, y) = locate(root, image, text)?;
    click_position(handle, cx, |size| point(size.width * x, size.height * y))?;
    println!("Clicked visible {text} at ({x:.3},{y:.3}).");
    Ok(())
}

/// Hit the center of a footer icon in logical window coordinates, just as a pointer does.
fn click_footer_icon(handle: AnyWindowHandle, cx: &mut AsyncApp, x: f32) -> Result<()> {
    click_position(handle, cx, |size| point(px(x), size.height - px(24.)))
}

fn click_position(handle: AnyWindowHandle, cx: &mut AsyncApp, target: impl FnOnce(Size<Pixels>) -> Point<Pixels>) -> Result<()> {
    handle.update(cx, |_, window, cx| {
        let size = window.viewport_size();
        let position = target(size);
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                position,
                button: MouseButton::Left,
                modifiers: Default::default(),
                click_count: 1,
            }),
            cx,
        );
    })?;
    Ok(())
}

async fn run(
    phase: &str,
    root: &Path,
    marker: &str,
    handle: AnyWindowHandle,
    wb: &Entity<KarWorkbench>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let store = ChatStore::open(root.join("chats"))?;
    if phase == "create-review" {
        wait_for(handle, wb, cx, "Codex readiness", |w| {
            w.providers.iter().any(|p| p.id == "codex" && p.is_ready())
        })
        .await?;
        let home = root.join("home.png");
        capture(handle, cx, &home).await?;
        click_footer_icon(handle, cx, 96.)?;
        let library = root.join("documents.png");
        capture(handle, cx, &library).await?;
        click_text(root, &library, "电子表格", handle, cx)?;
        wait_for(handle, wb, cx, "new native spreadsheet", |w| {
            w.active_office_tab
                .and_then(|i| w.office_tabs.get(i))
                .is_some_and(|tab| tab.kind == kar_desktop::types::OfficeKind::Excel)
                && w.main_stage_view == kar_desktop::workbench::MainStageView::Conversation
        })
        .await?;
        capture(handle, cx, &root.join("spreadsheet.png")).await?;
        handle.update(cx, |_, window, cx| {
            window.dispatch_keystroke(Keystroke::parse("cmd-j").unwrap(), cx);
        })?;
        handle.update(cx, |_, window, cx| {
            window.dispatch_keystroke(Keystroke::parse("cmd-n").unwrap(), cx);
            window.refresh();
            window.draw(cx).clear(cx);
        })?;
        let prompt = "Run exactly sh update-fixture.sh in the current workspace to update the two files. Reply only DONE.";
        type_and_send(handle, cx, prompt)?;
        let deadline = Instant::now() + Duration::from_secs(110);
        let mut approved = None;
        loop {
            let (done, request) = handle.update(cx, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                let state = &wb.read(cx).projected;
                (
                    state.turns.first().is_some_and(|t| {
                        t.status == TurnStatus::Completed
                            && t.checkpoint.as_ref().is_some_and(|c| c.files.len() == 2)
                    }),
                    state.pending_requests.first().cloned(),
                )
            })?;
            if done {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "timed out waiting for two-file checkpoint"
            );
            if let Some(request) = request
                && approved.as_ref() != Some(&request.cursor)
            {
                let req = serde_json::to_value(&request)?;
                let body = &req["body"];
                let command = body["command"].as_str().unwrap_or_default();
                ensure!(
                    body["cwd"].as_str().and_then(|p| fs::canonicalize(p).ok())
                        == Some(root.join("workspace").canonicalize()?)
                        && [
                            "sh update-fixture.sh",
                            "/bin/zsh -lc 'sh update-fixture.sh'",
                            "/bin/bash -lc 'sh update-fixture.sh'",
                            "/bin/sh -lc 'sh update-fixture.sh'"
                        ]
                        .contains(&command),
                    "unexpected approval for fixture: {command}"
                );
                let shot = root.join("00-permission.png");
                capture(handle, cx, &shot).await?;
                click_footer_icon(handle, cx, 132.)?;
                let inbox = root.join("inbox-request.png");
                capture(handle, cx, &inbox).await?;
                click_text(root, &inbox, "等待授权", handle, cx)?;
                let returned = root.join("inbox-to-request.png");
                capture(handle, cx, &returned).await?;
                ensure!(
                    handle.update(cx, |_, _, cx| wb
                        .read(cx)
                        .projected
                        .pending_requests
                        .iter()
                        .any(|r| r.cursor == request.cursor))?,
                    "request changed during navigation"
                );
                click_text(root, &returned, "允许一次", handle, cx)?;
                approved = Some(request.cursor);
            }
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
        }
        let id = handle
            .update(cx, |_, _, cx| wb.read(cx).projected.chat_id)?
            .context("chat id")?;
        ensure!(
            fs::read_to_string(root.join("workspace").join(FILE))?.trim() == marker,
            "first file"
        );
        ensure!(
            fs::read_to_string(root.join("workspace").join(OTHER))?.trim() == "appendix-ready",
            "second file"
        );
        fs::write(root.join("expected-chat"), id.to_string())?;
        capture(handle, cx, &root.join("01-changed.png")).await?;
        println!("Verified two real file edits in chat {id}.");
        return Ok(());
    }
    let expected = fs::read_to_string(root.join("expected-chat"))?.parse::<Uuid>()?;
    if phase == "draft-review" {
        wait_for(handle, wb, cx, "restore completed conversation", |w| {
            w.projected.chat_id == Some(expected)
                && w.projected.turns.len() == 2
                && w.git_snapshot.dirty_count() >= 2
        })
        .await?;
        let chat = root.join("06-resumed-again.png");
        capture(handle, cx, &chat).await?;
        click_footer_icon(handle, cx, 60.)?;
        wait_for(handle, wb, cx, "Git panel", |w| {
            w.right_surface == kar_desktop::workbench::RightPaneSurface::GitWorkbench
                && w.layout_mode != kar_desktop::workbench::WorkspaceLayoutMode::ChatFocus
        })
        .await?;
        let git = root.join("07-git.png");
        capture(handle, cx, &git).await?;
        click_text(root, &git, "起草审查请求", handle, cx)?;
        let draft = root.join("08-review-draft.png");
        capture(handle, cx, &draft).await?;
        locate(root, &draft, "请审查当前分支")?;
        ensure!(
            store.history(expected)?.turns.len() == 2,
            "draft action submitted a turn"
        );
        println!(
            "Verified the Git review action only fills the native composer and keeps both saved turns."
        );
        return Ok(());
    }
    wait_for(handle, wb, cx, "restore selected chat", |w| {
        w.projected.chat_id == Some(expected)
            && w.projected.turns.len() == 1
            && matches!(w.access_mode, ChatAccessMode::WriteController { .. })
    })
    .await?;
    let reopened = root.join("02-reopened.png");
    capture(handle, cx, &reopened).await?;
    click_text(root, &reopened, FILE, handle, cx)?;
    let diff = root.join("03-checkpoint.png");
    capture(handle, cx, &diff).await?;
    locate(root, &diff, marker)?;
    ensure!(
        locate(root, &diff, "later-worktree-only").is_err(),
        "showed working tree"
    );
    ensure!(
        locate(root, &diff, "appendix-ready").is_err(),
        "showed another file's diff"
    );
    click_text(root, &diff, "追问此改动", handle, cx)?;
    capture(handle, cx, &root.join("04-follow-up.png")).await?;
    type_and_send(
        handle,
        cx,
        "What exact value did you write to that file in the first turn? Reply only with that value.",
    )?;
    wait_for(handle, wb, cx, "follow-up reply", |w| {
        w.projected.chat_id == Some(expected)
            && w.projected
                .turns
                .get(1)
                .is_some_and(|t| t.status == TurnStatus::Completed)
    })
    .await?;
    let persisted = store.history(expected)?;
    ensure!(persisted.turns.len() == 2, "expected exactly two turns");
    ensure!(
        persisted.turns[1].items.iter().any(
            |i| matches!(&i.item,Item::Message{role:Role::Assistant,text,..} if text.trim()==marker)
        ),
        "reply did not reference the original file change"
    );
    ensure!(persisted.turns[1].items.iter().any(|i|matches!(&i.item,Item::Message{role:Role::User,text,..} if text.contains(FILE) && text.contains("第 1 轮"))),"missing file and turn context");
    capture(handle, cx, &root.join("05-answered.png")).await?;
    println!(
        "Verified resume, historical single-file diff and same-chat follow-up in {expected}, answer {marker}."
    );
    Ok(())
}
