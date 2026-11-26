//! Native schedule editor projection over Kar's project workflow service.
use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, bail, ensure};
use axum::http::Method;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    ScheduleCadence, ScheduleRegistry, ScheduleRunOutcome, ScheduleRunReceipt, ScheduleToneChoice,
    ScheduledTask,
};
use crate::{engine::Engine, remote_link::now_epoch_ms};

#[derive(Debug)]
pub enum Action {
    Refresh,
    Save {
        task: ScheduledTask,
        expected: Option<u64>,
    },
    Enable {
        id: Uuid,
        enabled: bool,
    },
    Run {
        id: Uuid,
        operation: Uuid,
    },
    Remove {
        id: Uuid,
    },
}

const TEMPLATE: &str = "\n\nKwrok Office · 打开此工作区文档作为任务上下文：\n";

fn cadence(value: &Value) -> Result<ScheduleCadence> {
    Ok(match value["type"].as_str() {
        Some("manual") => ScheduleCadence::ManualOnly,
        Some("webhook") => ScheduleCadence::Webhook {
            secret: String::new(),
        },
        Some("once") => ScheduleCadence::OnceAtMs(value["at_ms"].as_u64().context("at_ms")?),
        Some("interval") => ScheduleCadence::EveryMinutes(
            (value["every_ms"].as_u64().context("every_ms")? / 60_000).try_into()?,
        ),
        Some("fixed_time") => {
            let (h, m) = value["time"]
                .as_str()
                .context("time")?
                .split_once(':')
                .context("HH:MM")?;
            let mask = value["weekdays"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter(|d| *d < 7)
                .fold(0u8, |mask, day| mask | 1 << ((day + 6) % 7));
            match (h.parse()?, m.parse()?, mask) {
                (9, 0, 0 | 127) => ScheduleCadence::DailyNineAm,
                (9, 0, 1) => ScheduleCadence::WeeklyMonday,
                (hour, minute, weekdays_mask) => ScheduleCadence::CustomTime {
                    hour,
                    minute,
                    weekdays_mask,
                },
            }
        }
        _ => bail!("工作台返回了未知的调度类型"),
    })
}

fn timing(cadence: &ScheduleCadence) -> Value {
    match cadence {
        ScheduleCadence::ManualOnly => json!({"type":"manual"}),
        ScheduleCadence::Webhook { .. } => json!({"type":"webhook"}),
        ScheduleCadence::OnceAtMs(ms) => json!({"type":"once","at_ms":ms}),
        ScheduleCadence::EveryMinutes(m) => {
            json!({"type":"interval","every_ms":u64::from(*m)*60_000})
        }
        ScheduleCadence::WeeklyMonday => json!({"type":"fixed_time","time":"09:00","weekdays":[1]}),
        ScheduleCadence::DailyNineAm => json!({"type":"fixed_time","time":"09:00","weekdays":[]}),
        ScheduleCadence::CustomTime {
            hour,
            minute,
            weekdays_mask,
        } => {
            let days = (0..7)
                .filter(|day| weekdays_mask & (1 << day) != 0)
                .map(|day| (day + 1) % 7)
                .collect::<Vec<_>>();
            json!({"type":"fixed_time","time":format!("{hour:02}:{minute:02}"),"weekdays":days})
        }
    }
}

fn definition(task: &ScheduledTask, root: &Path) -> Result<Value> {
    ensure!(
        !task.title.trim().is_empty() && !task.prompt.trim().is_empty(),
        "请填写任务标题和指令"
    );
    let mut steps = Vec::new();
    if let Some(script) = &task.pre_script
        && !script.trim().is_empty()
    {
        steps.push(json!({"type":"script","command":script}));
    }
    let mut prompt = task.prompt.clone();
    if let Some(template) = &task.office_template {
        let path = template.canonicalize().context("找不到绑定的工作区文档")?;
        let relative = path
            .strip_prefix(root.canonicalize()?)
            .context("工作流只能读取本工作区内的文档模板")?;
        ensure!(path.is_file(), "模板必须是文件");
        prompt.push_str(TEMPLATE);
        prompt.push_str(relative.to_str().context("模板路径不是 UTF-8")?);
    }
    steps.push(json!({"type":"agent","runtime":task.runtime_id,"model":task.model,"effort":task.effort,"prompt":prompt}));
    let mut value = json!({"name":task.title,"steps":steps,"schedule":timing(&task.cadence),"enabled":task.enabled});
    if let ScheduleCadence::Webhook { secret } = &task.cadence
        && !secret.is_empty()
    {
        value["webhook_secret"] = json!(secret);
    }
    Ok(value)
}

async fn resolved_definition(engine: &Engine, task: &ScheduledTask, root: &Path) -> Result<Value> {
    let mut value = definition(task, root)?;
    if task.runtime_id == crate::aster::ID {
        let binary = crate::aster::find_binary().context("请先在智能体设置中安装 Aster")?;
        let runtime = crate::aster::register(engine, &binary).await?;
        if let Some(step) = value["steps"]
            .as_array_mut()
            .and_then(|steps| steps.last_mut())
        {
            step["runtime"] = json!(runtime);
        }
    }
    Ok(value)
}

fn uuid(field: &Value) -> Result<Uuid> {
    Ok(field.as_str().context("missing workflow ID")?.parse()?)
}

fn revision(workflow: &Value) -> u64 {
    let mut object = workflow.clone();
    if let Some(object) = object.as_object_mut() {
        object.remove("next_ms");
        object.remove("last_missed_ms");
    }
    let hash = ring::digest::digest(&ring::digest::SHA256, object.to_string().as_bytes());
    u64::from_be_bytes(hash.as_ref()[..8].try_into().unwrap())
}

fn project_task(w: &Value, root: &Path) -> Result<ScheduledTask> {
    let steps = w["steps"].as_array().context("missing workflow steps")?;
    let agent = steps.iter().find(|s| s["type"] == "agent");
    let schedule = &w["schedule"];
    let minute_interval = schedule["every_ms"]
        .as_u64()
        .is_none_or(|ms| ms % 60_000 == 0);
    let simple = matches!(steps.as_slice(), [a] if a["type"]=="agent")
        || matches!(steps.as_slice(), [s,a] if s["type"]=="script" && a["type"]=="agent");
    // Keep more advanced definitions intact. Their controls and run history stay available.
    let can_edit = simple
        && minute_interval
        && schedule["zone"].is_null()
        && ["effort", "service_tier", "subject"]
            .iter()
            .all(|k| w[*k].is_null())
        && w["tags"].as_array().is_none_or(Vec::is_empty)
        && w["workspace_strategy"]
            .as_str()
            .is_none_or(|s| s == "local")
        && w["concurrency"].as_str().is_none_or(|s| s == "allow")
        && agent.is_none_or(|a| a["service_tier"].is_null() && a["workspace_strategy"].is_null());
    let text = agent.and_then(|a| a["prompt"].as_str()).unwrap_or("");
    let (prompt, template) = text
        .rsplit_once(TEMPLATE)
        .map_or((text, None), |(p, t)| (p, Some(root.join(t))));
    Ok(ScheduledTask {
        id: uuid(&w["id"])?.to_string(),
        revision: revision(w),
        enabled: w["enabled"].as_bool().context("missing enabled")?,
        title: w["name"].as_str().context("missing name")?.into(),
        prompt: prompt.into(),
        cadence: cadence(schedule)?,
        runtime_id: agent
            .and_then(|a| a["runtime"].as_str())
            .unwrap_or("workflow")
            .into(),
        workspace: Some(root.to_path_buf()),
        model: agent.and_then(|a| a["model"].as_str()).map(String::from),
        effort: agent.and_then(|a| a["effort"].as_str()).map(String::from),
        office_template: template,
        pre_script: steps
            .first()
            .filter(|s| s["type"] == "script")
            .and_then(|s| s["command"].as_str())
            .map(String::from),
        tone: ScheduleToneChoice::Blue,
        next_due_ms: w["next_ms"].as_u64(),
        history: Vec::new(),
        engine_read_only: !can_edit,
    })
}

fn receipt(run: &Value) -> Result<ScheduleRunReceipt> {
    let agent = run["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| step["type"] == "agent");
    let chat = run["attempt"]["chat"]
        .as_str()
        .or_else(|| {
            run["receipts"]
                .as_array()?
                .iter()
                .rev()
                .find_map(|r| r["attempt"]["chat"].as_str())
        })
        .map(str::parse)
        .transpose()?;
    Ok(ScheduleRunReceipt {
        request_id: uuid(&run["id"])?,
        chat_id: chat,
        triggered_ms: run["started_ms"].as_u64().unwrap_or_default(),
        manual: run["trigger"]["type"] == "manual",
        captured_runtime: agent
            .and_then(|a| a["runtime"].as_str())
            .unwrap_or("workflow")
            .into(),
        captured_effort: agent.and_then(|a| a["effort"].as_str()).map(String::from),
        outcome: match run["status"].as_str() {
            Some("completed") => ScheduleRunOutcome::Completed,
            Some("failed" | "cancelled" | "stopped" | "skipped") => ScheduleRunOutcome::Failed,
            Some("ready" | "running" | "waiting" | "asking") => ScheduleRunOutcome::Admitted,
            _ => ScheduleRunOutcome::OutcomeUnknown,
        },
    })
}

// Old schedules remain readable for historical receipts. A marker makes migration
// restartable, and prevents a deleted workflow from being resurrected on refresh.
async fn migrate(
    engine: &Engine,
    root: &Path,
    store: &Path,
    fallback: &Path,
    base: &str,
) -> Result<(BTreeMap<Uuid, ScheduledTask>, Vec<String>)> {
    let legacy = ScheduleRegistry::read(store, now_epoch_ms())?;
    let mut tasks = BTreeMap::new();
    let mut warnings = Vec::new();
    for task in legacy.tasks {
        if task
            .workspace
            .as_deref()
            .unwrap_or(fallback)
            .canonicalize()
            .ok()
            .as_deref()
            == Some(root)
        {
            tasks.insert(Uuid::new_v5(&engine.project_id(), task.id.as_bytes()), task);
        }
    }
    if tasks.is_empty() {
        return Ok((tasks, warnings));
    }
    let dir = store
        .join("desktop-workflow-migration")
        .join(engine.project_id().to_string());
    std::fs::create_dir_all(&dir)?;
    let pending = tasks
        .iter()
        .filter(|(id, _)| !dir.join(id.to_string()).exists())
        .collect::<Vec<_>>();
    if !pending.is_empty() {
        let items = engine.request(Method::GET, base, None).await?;
        for (&id, task) in pending {
            let result: Result<()> = async {
                let pending = dir.join(format!("{id}.pending"));
                let mut migrated = task.clone();
                // Only the original enable bit needs journaling. Existing schedule credentials
                // stay in their original store. A crash after disabling the old ticker can resume.
                if let Ok(bit) = std::fs::read_to_string(&pending) {
                    migrated.enabled = bit == "1";
                }
                if matches!(migrated.cadence, ScheduleCadence::OnceAtMs(t) if t <= now_epoch_ms()) {
                    migrated.enabled = false;
                }
                let data = resolved_definition(engine, &migrated, root).await?;
                if !pending.exists() {
                    let temp = dir.join(format!("{id}.tmp"));
                    std::fs::write(&temp, if task.enabled { "1" } else { "0" })?;
                    std::fs::rename(temp, &pending)?;
                }
                if task.enabled {
                    ScheduleRegistry::default().transact(store, now_epoch_ms(), |reg| {
                        reg.set_enabled(&task.id, false, now_epoch_ms()).map(|_| ())
                    })?;
                }
                if !items
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|w| uuid(&w["id"]).ok() == Some(id))
                {
                    engine
                        .request(Method::PUT, &format!("{base}/{id}"), Some(data))
                        .await?;
                }
                std::fs::rename(pending, dir.join(id.to_string()))?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                warnings.push(format!("「{}」尚未迁移：{error:#}", task.title));
            }
        }
    }
    Ok((tasks, warnings))
}

pub(crate) async fn apply(
    engine: &Engine,
    workspace: &Path,
    store: &Path,
    fallback: &Path,
    action: Action,
) -> Result<ScheduleRegistry> {
    static EDIT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _edit = EDIT.lock().await;
    let root = workspace.canonicalize()?;
    let project = engine.project_id();
    let base = format!("/projects/{project}/workflows");
    let (old, migration_warnings) = migrate(engine, &root, store, fallback, &base).await?;
    match action {
        Action::Refresh => {}
        Action::Save { task, expected } => {
            let id: Uuid = task.id.parse()?;
            if let Some(expected) = expected {
                let defs = engine.request(Method::GET, &base, None).await?;
                let w = defs
                    .as_array()
                    .context("missing workflows")?
                    .iter()
                    .find(|w| uuid(&w["id"]).ok() == Some(id))
                    .context("任务已被移除")?;
                ensure!(
                    revision(w) == expected && !project_task(w, &root)?.engine_read_only,
                    "任务配置已更新，请刷新后再编辑"
                );
            }
            engine
                .request(
                    Method::PUT,
                    &format!("{base}/{id}"),
                    Some(resolved_definition(engine, &task, &root).await?),
                )
                .await?;
        }
        Action::Enable { id, enabled } => {
            engine
                .request(
                    Method::PUT,
                    &format!("{base}/{id}/enabled"),
                    Some(json!({"enabled":enabled})),
                )
                .await?;
        }
        Action::Run { id, operation } => {
            engine
                .request(
                    Method::POST,
                    &format!("{base}/{id}/runs"),
                    Some(json!({"operation":operation})),
                )
                .await?;
        }
        Action::Remove { id } => {
            engine
                .request(Method::DELETE, &format!("{base}/{id}"), None)
                .await?;
        }
    }
    let state = engine
        .request(
            Method::GET,
            &format!("/projects/{project}/workflow-state"),
            None,
        )
        .await?;
    let mut tasks = Vec::new();
    for w in state["workflows"]
        .as_array()
        .context("missing workflow list")?
    {
        let id = uuid(&w["id"])?;
        let mut task = project_task(w, &root)?;
        if state["runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| r["workflow"] == w["id"])
        {
            let runs = engine
                .request(Method::GET, &format!("{base}/{id}/runs"), None)
                .await?;
            for r in runs.as_array().context("missing workflow runs")? {
                task.history.push(receipt(r)?);
            }
        }
        if let Some(old) = old.get(&id) {
            task.tone = old.tone;
            task.history.extend(old.history.clone());
        }
        task.history.sort_by_key(|r| r.triggered_ms);
        tasks.push(task);
    }
    Ok(ScheduleRegistry {
        migration_warnings,
        webhook_base: if tasks
            .iter()
            .any(|t| matches!(t.cadence, ScheduleCadence::Webhook { .. }))
        {
            Some(format!(
                "{}/hooks/workflows/{project}",
                engine.loopback_url()?
            ))
        } else {
            None
        },
        revision: state["revision"].as_u64().unwrap_or_default(),
        seeded_defaults: false,
        tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kar::chat::ChatStore;

    #[tokio::test]
    async fn migrated_schedule_uses_one_engine_and_preserves_history_after_deletion() -> Result<()>
    {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("project");
        let state = tmp.path().join("chats");
        std::fs::create_dir(&root)?;
        let engine = Engine::open(ChatStore::open(&state)?, &root)?;
        let mut old: ScheduledTask = serde_json::from_value(json!({
            "id":"sched-daily","revision":1,"title":"Legacy","prompt":"Read the notes",
            "enabled":true,"runtime_id":"codex","tone":"blue","cadence":"daily_nine_am"
        }))?;
        let prior = Uuid::new_v4();
        old.history.push(ScheduleRunReceipt {
            request_id: prior,
            chat_id: None,
            triggered_ms: 4,
            manual: true,
            captured_runtime: "codex".into(),
            captured_effort: None,
            outcome: ScheduleRunOutcome::Completed,
        });
        let old = ScheduleRegistry {
            tasks: vec![old],
            ..Default::default()
        };
        old.save(&state)?;
        let registry = apply(&engine, &root, &state, &root, Action::Refresh).await?;
        assert!(
            registry.migration_warnings.is_empty(),
            "{:?}",
            registry.migration_warnings
        );
        assert_eq!(registry.tasks.len(), 1);
        let task = &registry.tasks[0];
        assert!(task.enabled);
        assert!(!task.engine_read_only);
        assert_eq!(task.history[0].request_id, prior);
        assert!(!ScheduleRegistry::read(&state, now_epoch_ms())?.tasks[0].enabled);
        let id = task.id.parse()?;
        let mut updated = task.clone();
        updated.title = "Edited".into();
        let edited = apply(
            &engine,
            &root,
            &state,
            &root,
            Action::Save {
                task: updated.clone(),
                expected: Some(task.revision),
            },
        )
        .await?;
        assert_eq!(edited.tasks[0].title, "Edited");
        assert!(
            apply(
                &engine,
                &root,
                &state,
                &root,
                Action::Save {
                    task: updated,
                    expected: Some(task.revision)
                }
            )
            .await
            .is_err()
        );
        let disabled = apply(
            &engine,
            &root,
            &state,
            &root,
            Action::Enable { id, enabled: false },
        )
        .await?;
        assert!(!disabled.tasks[0].enabled);
        assert!(
            apply(&engine, &root, &state, &root, Action::Remove { id })
                .await?
                .tasks
                .is_empty()
        );
        assert!(
            apply(&engine, &root, &state, &root, Action::Refresh)
                .await?
                .tasks
                .is_empty()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn real_workflow_clock_and_manual_retry_write_once_and_report_their_runs() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("project");
        let state = tmp.path().join("chats");
        std::fs::create_dir(&root)?;
        let engine = Engine::open(ChatStore::open(&state)?, &root)?;
        let project = engine.project_id();
        let manual = Uuid::new_v4();
        let clock = Uuid::new_v4();
        for (id, schedule, file) in [
            (manual, json!({"type":"manual"}), "manual.txt"),
            (
                clock,
                json!({"type":"once","at_ms": now_epoch_ms()+1_500}),
                "clock.txt",
            ),
        ] {
            engine.request(Method::PUT,&format!("/projects/{project}/workflows/{id}"),
                Some(json!({"name":file,"enabled":true,"schedule":schedule,"steps":[{"type":"script","command":format!("printf x >> {file}")}]}))).await?;
        }
        let operation = Uuid::new_v4();
        for _ in 0..2 {
            apply(
                &engine,
                &root,
                &state,
                &root,
                Action::Run {
                    id: manual,
                    operation,
                },
            )
            .await?;
        }
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let r=apply(&engine,&root,&state,&root,Action::Refresh).await?;
                if r.tasks.iter().all(|t| matches!(t.history.as_slice(),[receipt] if receipt.outcome == ScheduleRunOutcome::Completed)) {
                    assert_eq!(r.tasks.iter().find(|t|t.id==manual.to_string()).unwrap().history[0].manual,true);
                    assert_eq!(r.tasks.iter().find(|t|t.id==clock.to_string()).unwrap().history[0].manual,false);
                    return Ok::<(),anyhow::Error>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }).await??;
        for file in ["manual.txt", "clock.txt"] {
            assert_eq!(std::fs::read(root.join(file))?, b"x");
        }
        Ok(())
    }
    #[tokio::test]
    async fn webhook_url_hits_real_kar_signature_verification_and_delivery() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("workspace");
        let state = tmp.path().join("chats");
        std::fs::create_dir(&root)?;
        let engine = Engine::open(ChatStore::open(&state)?, &root)?;
        let project = engine.project_id();
        let id = Uuid::new_v4();
        let secret = Uuid::new_v4().to_string();
        engine.request(Method::PUT,&format!("/projects/{project}/workflows/{id}"),Some(json!({
            "name":"Hook test","enabled":true,"schedule":{"type":"webhook"},"webhook_secret":secret,
            "steps":[{"type":"stop","reason":"Delivery arrived"}]
        }))).await?;
        let reg = apply(&engine, &root, &state, &root, Action::Refresh).await?;
        let url = format!("{}/{id}", reg.webhook_base.context("missing webhook URL")?);
        let data = br#"{"id":"desktop-event","input":{"issue":42}}"#;
        let client = reqwest::Client::new();
        assert_eq!(
            client
                .post(&url)
                .body(data.as_slice())
                .send()
                .await?
                .status()
                .as_u16(),
            401
        );
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
        let sig = ring::hmac::sign(&key, data)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let response = client
            .post(url)
            .header("x-kar-signature", format!("sha256={sig}"))
            .header("x-kar-delivery", Uuid::new_v4().to_string())
            .header("content-type", "application/json")
            .body(data.as_slice())
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "webhook delivery: {}",
            response.status()
        );
        let history = engine
            .request(
                Method::GET,
                &format!("/projects/{project}/workflows/{id}/runs"),
                None,
            )
            .await?;
        assert_eq!(history.as_array().unwrap().len(), 1);
        assert_eq!(history[0]["trigger"]["type"], "webhook");
        Ok(())
    }
}
