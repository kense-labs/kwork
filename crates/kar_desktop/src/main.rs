extern crate gpui_kit as gpui;

use std::path::PathBuf;
use std::time::Duration;

use gpui::*;
use kar_desktop::{
    SmokeIntegrationView,
    coedit::{ScenarioKey, UserResolutionChoice},
    init_runtime_ui,
    workbench::{KarWorkbench, WorkbenchConfig},
};

const USAGE: &str = "\
kar-desktop — Pure-Rust GPUI desktop client for Kar + Kantor Office (built with KUI)

USAGE:
  kar-desktop [OPTIONS] [FILE...]

OPTIONS:
  --chat-dir <DIR>             Isolated KAR_CHAT_DIR for ChatStore (default: $KAR_CHAT_DIR or ./.kar-desktop-chats)
  --workspace <DIR>            Default workspace directory for new Kar chats
  --docs-dir <DIR>             Working copies directory for Office sample documents
  --open <FILE>                Open a .docx, .xlsx, or .pptx copy in the Office pane (repeatable)
  --subscribe-http <URL>       Start in HTTP Read-Only Subscriber mode (GET /snapshot + GET /events)
  --demo-chat                  Start in independent Demo Chat mode (clearly marked 演示数据)
  --coedit-demo                Open the isolated Human-AI Co-Editing Demo Workshop
  --coedit-scenario <NAME>     Select initial co-edit scenario: word | excel | ppt
  --coedit-step <STEP>         Optional scripted co-edit step (e.g. word_human_date, excel_human_b3, ppt_human_move, ppt_human_delete, excel_mid_conflict_edit)
  --coedit-resolve <CHOICE>    Optional scripted resolution: dual | keep | adopt | manual:<VAL> | dismiss
  --bind-selection             Capture active Office tab selection into the Chat composer on startup
  --switch-tab <INDEX>         Switch active Office tab (0-based) after binding selection
  --select-runtime <ID>        Pre-select runtime in sidebar (e.g. codex, grok, claude)
  --dark                       Launch in unified Dark theme (KUI + Kantor)
  --smoke-integration          Launch the minimal Stage-1 KUI + Kantor same-window smoke view
  --screenshot <PNG_PATH>      Render native Metal window to PNG after delay and exit cleanly
  --screenshot-delay-ms <MS>   Delay before capturing --screenshot (default: 1200)
  -h, --help                   Print this help message
";

struct CliOptions {
    workbench: WorkbenchConfig,
    smoke_integration: bool,
    coedit_steps: Vec<&'static str>,
    coedit_resolve: Option<UserResolutionChoice>,
    bind_selection: bool,
    switch_tab_after_bind: Option<usize>,
    select_runtime: Option<String>,
    select_cell: Option<String>,
    screenshot_out: Option<PathBuf>,
    screenshot_delay_ms: u64,
}

fn leak_known_step(s: &str) -> Option<&'static str> {
    match s {
        "word_human_date" => Some("word_human_date"),
        "word_mid_conflict_edit" => Some("word_mid_conflict_edit"),
        "excel_human_b3" => Some("excel_human_b3"),
        "excel_mid_conflict_edit" => Some("excel_mid_conflict_edit"),
        "ppt_human_move" => Some("ppt_human_move"),
        "ppt_mid_conflict_move" => Some("ppt_mid_conflict_move"),
        "ppt_human_delete" => Some("ppt_human_delete"),
        "reset_all" => Some("reset_all"),
        _ => None,
    }
}

fn parse_cli() -> CliOptions {
    let mut wb = WorkbenchConfig::default();
    let mut smoke_integration = false;
    let mut coedit_steps = Vec::new();
    let mut coedit_resolve = None;
    let mut bind_selection = false;
    let mut switch_tab_after_bind = None;
    let mut select_runtime = None;
    let mut select_cell = None;
    let mut screenshot_out = None;
    let mut screenshot_delay_ms = 1200u64;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--chat-dir" => {
                if let Some(v) = args.next() {
                    wb.chat_dir = PathBuf::from(v);
                }
            }
            "--workspace" => {
                if let Some(v) = args.next() {
                    wb.workspace_dir = PathBuf::from(v);
                }
            }
            "--docs-dir" => {
                if let Some(v) = args.next() {
                    wb.docs_dir = PathBuf::from(v);
                }
            }
            "--open" => {
                if let Some(v) = args.next() {
                    wb.open_files.push(PathBuf::from(v));
                }
            }
            "--subscribe-http" => {
                wb.subscribe_http = args.next();
            }
            "--demo-chat" => {
                wb.start_demo_chat = true;
            }
            "--coedit-demo" => {
                wb.start_coedit_demo = true;
            }
            "--coedit-scenario" => {
                if let Some(v) = args.next() {
                    wb.start_coedit_demo = true;
                    wb.initial_coedit_scenario = match v.as_str() {
                        "excel" => Some(ScenarioKey::ExcelSameCellConflict),
                        "ppt" => Some(ScenarioKey::PptMoveAndText),
                        _ => Some(ScenarioKey::WordSameParagraph),
                    };
                }
            }
            "--coedit-step" => {
                if let Some(v) = args.next()
                    && let Some(st) = leak_known_step(&v)
                {
                    coedit_steps.push(st);
                }
            }
            "--coedit-resolve" => {
                if let Some(v) = args.next() {
                    coedit_resolve = match v.as_str() {
                        "dual" => Some(UserResolutionChoice::ApplyDualKeep),
                        "keep" => Some(UserResolutionChoice::KeepCurrent),
                        "adopt" => Some(UserResolutionChoice::AdoptProposal),
                        "dismiss" => Some(UserResolutionChoice::DismissDeletedTarget),
                        s if s.starts_with("manual:") => Some(UserResolutionChoice::ManualMerge {
                            merged_value: s["manual:".len()..].to_owned(),
                        }),
                        _ => None,
                    };
                }
            }
            "--bind-selection" => {
                bind_selection = true;
            }
            "--switch-tab" => {
                if let Some(v) = args.next() {
                    switch_tab_after_bind = v.parse::<usize>().ok();
                }
            }
            "--select-runtime" => {
                select_runtime = args.next();
            }
            "--select-cell" => {
                select_cell = args.next();
            }
            "--dark" => {
                wb.dark_mode = true;
            }
            "--smoke-integration" => {
                smoke_integration = true;
            }
            "--screenshot" => {
                screenshot_out = args.next().map(PathBuf::from);
            }
            "--screenshot-delay-ms" => {
                if let Some(v) = args.next()
                    && let Ok(ms) = v.parse::<u64>()
                {
                    screenshot_delay_ms = ms;
                }
            }
            other if !other.starts_with('-') => {
                wb.open_files.push(PathBuf::from(other));
            }
            other => {
                eprintln!("kar-desktop: ignoring unknown argument `{other}`");
            }
        }
    }

    CliOptions {
        workbench: wb,
        smoke_integration,
        coedit_steps,
        coedit_resolve,
        bind_selection,
        switch_tab_after_bind,
        select_runtime,
        select_cell,
        screenshot_out,
        screenshot_delay_ms,
    }
}

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,kar_desktop=info"),
    )
    .init();

    let opts = parse_cli();

    let app = gpui_kit::application().with_assets(kantor_ui::KantorAssets);
    app.run(move |cx| {
        init_runtime_ui(cx);

        let window_options = WindowOptions {
            app_id: Some("kar-desktop".into()),
            titlebar: Some(TitlebarOptions {
                title: Some("Kar Studio — Native GPUI Workbench & Kantor Office".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1480.), px(900.)),
                cx,
            ))),
            window_min_size: Some(size(px(960.), px(600.))),
            ..Default::default()
        };

        if opts.smoke_integration {
            let sample = opts.workbench.open_files.first().cloned();
            let (window, _) = match gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| SmokeIntegrationView::new(sample, window, cx))
            }) {
                Ok(o) => o,
                Err(err) => {
                    eprintln!("kar-desktop: failed to open window: {err:#}");
                    std::process::exit(1);
                }
            };
            cx.activate(true);
            schedule_screenshot(window, opts.screenshot_out, opts.screenshot_delay_ms, cx);
            return;
        }

        let wb_cfg = opts.workbench.clone();
        let (window, entity) = match gpui_kit::open_window(window_options, cx, move |window, cx| {
            cx.new(|cx| KarWorkbench::new(wb_cfg, window, cx))
        }) {
            Ok(o) => o,
            Err(err) => {
                eprintln!("kar-desktop: failed to open window: {err:#}");
                std::process::exit(1);
            }
        };
        cx.activate(true);

        // Optional scripted UI steps (used for deterministic verification & screenshot capture)
        let steps = opts.coedit_steps.clone();
        let resolve = opts.coedit_resolve.clone();
        let bind_sel = opts.bind_selection;
        let switch_tab = opts.switch_tab_after_bind;
        let sel_rt = opts.select_runtime.clone();
        let sel_cell = opts.select_cell.clone();
        if !steps.is_empty()
            || resolve.is_some()
            || bind_sel
            || switch_tab.is_some()
            || sel_rt.is_some()
            || sel_cell.is_some()
        {
            let win_handle = window;
            let wb_entity = entity;
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(650))
                    .await;
                let _ = win_handle.update(cx, |_, window, cx| {
                    wb_entity.update(cx, |wb, cx| {
                        if let Some(ref rt_id) = sel_rt {
                            wb.select_runtime_by_id(rt_id, cx);
                        }
                        if let Some(ref a1) = sel_cell {
                            wb.select_excel_cell_on_active_tab(a1, window, cx);
                        }
                        if bind_sel {
                            wb.bind_active_selection_to_chat(window, cx);
                        }
                        if let Some(tab_i) = switch_tab
                            && tab_i < wb.office_tabs.len()
                        {
                            wb.active_office_tab = Some(tab_i);
                            cx.notify();
                        }
                        for st in &steps {
                            wb.trigger_coedit_human_step(st, window, cx);
                        }
                        if let Some(ref choice) = resolve {
                            let sc = wb.active_coedit_scenario;
                            if let Some(op_id) = wb
                                .coedit_engine
                                .proposals()
                                .iter()
                                .find(|p| p.scenario == sc)
                                .map(|p| p.operation_id)
                            {
                                wb.submit_coedit_choice(op_id, choice.clone(), window, cx);
                            }
                        }
                    });
                });
            })
            .detach();
        }

        schedule_screenshot(window, opts.screenshot_out, opts.screenshot_delay_ms, cx);
    });
}

fn schedule_screenshot(
    window: AnyWindowHandle,
    screenshot_out: Option<PathBuf>,
    delay_ms: u64,
    cx: &mut App,
) {
    #[cfg(target_os = "macos")]
    if let Some(out) = screenshot_out {
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay_ms))
                .await;
            let _ = window.update(cx, |_, window, cx| {
                match window.render_to_image() {
                    Ok(img) => {
                        if let Some(parent) = out.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(err) = img.save(&out) {
                            eprintln!("failed to save screenshot {}: {err}", out.display());
                        } else {
                            println!("saved screenshot: {}", out.display());
                        }
                    }
                    Err(err) => eprintln!("render_to_image failed: {err:#}"),
                }
                cx.quit();
            });
        })
        .detach();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, screenshot_out, delay_ms, cx);
}
