//! Real Kar service and Git transport against a loopback-only Forge provider.
use super::*;
use axum::{
    Json, Router,
    extract::{Path as Param, State},
    http::StatusCode,
    routing::{get, post, put},
};
use std::{
    fs,
    process::Command,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Fixture {
    pulls: Vec<Value>,
    merged: Vec<Value>,
}
type Shared = Arc<Mutex<Fixture>>;

async fn post_pull(State(db): State<Shared>, Json(input): Json<Value>) -> Json<Value> {
    let mut db = db.lock().unwrap();
    let number = db.pulls.len() + 1;
    let pull = json!({"number":number,"state":"open","title":input["title"],"body":input["body"],"draft":input["draft"],"user":{"login":"author"},"html_url":format!("https://review.example/team/repo/pull/{number}"),"head":{"ref":input["head"],"sha":"reviewed-sha","repo":{"full_name":"team/repo"}},"base":{"ref":input["base"],"sha":"base-sha"}});
    db.pulls.push(pull.clone());
    Json(pull)
}
async fn merge_pull(State(db): State<Shared>, Json(input): Json<Value>) -> Json<Value> {
    let mut db = db.lock().unwrap();
    db.merged.push(input.clone());
    Json(json!({"merged": input["sha"]=="reviewed-sha" && input["merge_method"]=="squash"}))
}
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    ensure!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8(out.stdout)?.trim().into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_publication_requires_the_bound_chat_and_reviewed_head() -> Result<()> {
    const CHILD: &str = "KWORK_TEST_FORGE_ROOT";
    if let Some(root) = std::env::var_os(CHILD) {
        return exercise(std::path::PathBuf::from(root).as_path()).await;
    }
    let temp = tempfile::tempdir()?;
    let db: Shared = Default::default();
    let api=Router::new()
        .route("/user",get(||async {Json(json!({"login":"tester","id":1}))}))
        .route("/repos/team/repo",get(||async {Json(json!({"full_name":"team/repo","default_branch":"main","permissions":{"push":true}}))}))
        .route("/repos/team/repo/pulls",get(|State(db):State<Shared>|async move {Json(json!(db.lock().unwrap().pulls))}).post(post_pull))
        .route("/repos/team/repo/pulls/{num}",get(|State(db):State<Shared>,Param(n):Param<usize>|async move {db.lock().unwrap().pulls.get(n.saturating_sub(1)).cloned().map(Json).ok_or(StatusCode::NOT_FOUND)}))
        .route("/repos/team/repo/pulls/{num}/merge",put(merge_pull))
        .route("/graphql",post(||async {Json(json!({"data":{"search":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"repository":{"nameWithOwner":"team/repo"},"number":72,"title":"Threaded issues","state":"OPEN","createdAt":"2026-10-01T12:00:00Z","updatedAt":"2026-11-02T12:00:00Z","url":"https://review.example/team/repo/issues/72","author":{"login":"reviewer"}}]}}}))}))
        .with_state(db.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, api).await });
    let config = temp.path().join("test-forge.toml");
    fs::write(
        &config,
        format!(
            "[[hosts]]\nhost='review.example'\nkind='github'\napi_base='http://{addr}'\ntoken_env='KWORK_LOCAL_TEST_TOKEN'\n"
        ),
    )?;
    // Isolate Kar's process-scoped Forge host configuration from parallel tests.
    let root = temp.path().to_owned();
    let child = tokio::task::spawn_blocking(move || {
        Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "engine::forge::tests::local_publication_requires_the_bound_chat_and_reviewed_head",
                "--nocapture",
            ])
            .env(CHILD, root)
            .env("KAR_FORGE_CONFIG", config)
            .env("KWORK_LOCAL_TEST_TOKEN", "fixture")
            .status()
    })
    .await??;
    server.abort();
    ensure!(child.success(), "isolated Forge client failed");
    let db = db.lock().unwrap();
    assert_eq!(db.pulls.len(), 2);
    assert_eq!(
        db.merged.as_slice(),
        [
            json!({"sha":"reviewed-sha","merge_method":"merge"}),
            json!({"sha":"reviewed-sha","merge_method":"squash"})
        ]
    );
    Ok(())
}

async fn exercise(dir: &Path) -> Result<()> {
    let repo = dir.join("repo");
    let bare = dir.join("bare");
    fs::create_dir(&repo)?;
    fs::create_dir(&bare)?;
    git(&bare, &["init", "--bare"])?;
    git(&repo, &["init", "-b", "main"])?;
    git(&repo, &["config", "user.name", "fixture"])?;
    git(&repo, &["config", "user.email", "fixture@localhost"])?;
    git(&repo, &["config", "commit.gpgsign", "false"])?;
    fs::write(repo.join("notes"), "reviewed content")?;
    git(&repo, &["add", "notes"])?;
    git(&repo, &["commit", "-m", "fixture"])?;
    git(&repo, &["checkout", "-b", "topic"])?;
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://review.example/team/repo.git",
        ],
    )?;
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            bare.to_str().context("non-UTF8 test path")?,
        ],
    )?;
    let store = kar::chat::ChatStore::open(dir.join("store"))?;
    let id = Uuid::new_v4();
    let recording = dir.join("recording");
    fs::write(&recording,json!({"stream_id":id,"seq":0,"timestamp_ms":1,"direction":"to_runtime","kind":"request","session_id":id.to_string(),"native":{"method":"turn/start","params":{"input":[{"type":"text","text":"fixture"}]}},"events":[]}).to_string())?;
    store.import_recording("codex", &recording, &repo, None)?;
    let engine = Engine::open(store, &repo)?;
    let op = Uuid::new_v4();
    ensure!(
        create(
            &engine,
            &repo,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "unbound",
            "",
            "main",
            false
        )
        .await
        .is_err(),
        "unbound write succeeded"
    );
    let published = create(&engine, &repo, op, id, "Ready", "Details", "main", false).await?;
    ensure!(published.contains("#1"), "{published}");
    ensure!(git(&bare, &["show", "refs/heads/topic:notes"])? == "reviewed content");
    assert_eq!(
        create(&engine, &repo, op, id, "Ready", "Details", "main", false).await?,
        published
    );
    git(&repo, &["checkout", "-b", "draft"])?;
    let draft = create(
        &engine,
        &repo,
        Uuid::new_v4(),
        id,
        "Draft",
        "Details",
        "main",
        true,
    )
    .await?;
    ensure!(draft.contains("#2"), "{draft}");
    let (prs, issues, error) = read(&engine).await;
    ensure!(error.is_none(), "{error:?}");
    ensure!(prs.len() == 2 && prs.iter().any(|p| p.number == 2 && p.is_draft));
    ensure!(
        issues
            .iter()
            .any(|i| i.number_label == "#72" && i.author == "reviewer")
    );
    let sha = &prs
        .iter()
        .find(|p| p.number == 1)
        .context("missing PR")?
        .head_sha;
    ensure!(
        merge(
            &engine,
            Uuid::new_v4(),
            id,
            1,
            "stale-sha",
            PrMergeMethod::Squash
        )
        .await
        .is_err()
    );
    ensure!(
        merge(
            &engine,
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            sha,
            PrMergeMethod::Squash
        )
        .await
        .is_err()
    );
    ensure!(
        merge(&engine, Uuid::new_v4(), id, 1, sha, PrMergeMethod::Merge)
            .await
            .is_err(),
        "provider refused the merge"
    );
    let merge_op = Uuid::new_v4();
    assert_eq!(
        merge(&engine, merge_op, id, 1, sha, PrMergeMethod::Squash).await?,
        MergeOutcome::Merged
    );
    assert_eq!(
        merge(&engine, merge_op, id, 1, sha, PrMergeMethod::Squash).await?,
        MergeOutcome::Merged
    );
    Ok(())
}
