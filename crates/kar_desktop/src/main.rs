#![forbid(unsafe_code)]

extern crate gpui_kit as gpui;

use std::path::PathBuf;
use std::time::Duration;

use gpui::*;
use kar_desktop::{
    APP_NAME, SmokeIntegrationView,
    coedit::{ScenarioKey, UserResolutionChoice},
    init_runtime_ui,
    kar_bridge::KarCommand,
    studio_window_options,
    ui_settings::SettingsTab,
    workbench::{KarWorkbench, Menu, WorkbenchConfig, WorkspaceLayoutMode},
};

const USAGE: &str = "\
Kwrok — native GPUI desktop for Kar conversations and Kantor Office

USAGE:
  kar-desktop [OPTIONS] [FILE...]

OPTIONS:
  --chat-dir <DIR>             Override chat storage ($KAR_CHAT_DIR, otherwise app data or ./.kar-desktop-chats)
  --workspace <DIR>            Default workspace directory for new conversations
  --docs-dir <DIR>             Working copies directory for Office sample documents
  --open <FILE>                Open a .docx, .xlsx, or .pptx copy in the Office pane (repeatable)
  --subscribe-http <URL>       Start in HTTP Read-Only Subscriber mode (GET /snapshot + GET /events)
  --demo-chat                  Start in independent Demo Chat mode (clearly marked 演示数据)
  --brief-demo                 Seed the native GPUI + KUI Interactive Brief showcase card
  --guidance-demo              Seed smooth in-turn Guidance Queue Tray + 30s backgrounded command strip
  --coedit-demo                Open the isolated Human-AI Co-Editing Demo Workshop
  --coedit-scenario <NAME>     Select initial co-edit scenario: word | excel | ppt
  --coedit-step <STEP>         Optional scripted co-edit step (e.g. word_human_date, excel_human_b3, ppt_human_move, ppt_human_delete, excel_mid_conflict_edit)
  --coedit-resolve <CHOICE>    Optional scripted resolution: dual | keep | adopt | manual:<VAL> | dismiss
  --bind-selection             Capture active Office tab selection into the Chat composer on startup
  --switch-tab <INDEX>         Switch active Office tab (0-based) after binding selection
  --select-runtime <ID>        Pre-select runtime (e.g. codex, grok, claude)
  --open-cmd-palette           Open the ⌘K KUI CommandDesk overlay after launch
  --open-settings <TAB>        Open the unified KUI Settings Center (general | agents | skills | usage | voice | schedules | remote | logs)
  --open-provider-health       Open Settings -> Agents & Accounts tab
  --open-remote-sheet          Open Settings -> Remote Pairing tab
  --open-schedules-sheet       Open Settings -> Scheduled Tasks tab
  --open-context-popover       Open the context usage inspector after launch
  --open-git-tab               Open the native Git Changes & Diff Workbench in the right dock
  --open-git-graph             Open the native Git BranchGraph view in the right dock
  --expand-git-diff            Expand the first modified file's inline PatchReview diff in the Git dock
  --open-files-tab             Open the Workspace File Tree in the right dock
  --open-browser               Open the Built-in System WebView Browser in the right dock
  --browser-url <URL>          Open <URL> inside Kwrok's Built-in Browser on launch
  --open-terminal-tab          Open the Integrated Workspace Terminal in the right dock
  --run-terminal <CMD>         Run an initial command inside the Integrated Workspace Terminal
  --enable-worktree-mode       Pre-toggle new chat isolation mode to Git Worktree
  --open-skills-sheet          Open Settings -> Skills & MCP tab
  --open-usage-sheet           Open Settings -> Usage & Cache tab
  --open-handoff-sheet         Open the Cross-Runtime Session Handoff modal
  --open-side-question         Open the Ephemeral Side Question sheet
  --open-logs-sheet            Open Settings -> Diagnostics LogStream tab
  --sidebar-files              Open the Workspace Repo Files TreeView in the right dock
  --open-dropzone              Expand the Start Stage KUI FileDropzone tray
  --brief-rev <REV>            Switch the active Brief showcase card to revision <REV> (e.g. 1 or 2)
  --open-runtime-popover       Open the inline composer runtime picker popover after launch
  --open-latest-chat           Reopen the most recent saved chat in --chat-dir on launch
  --collapse-sidebar           Collapse the left navigation rail after launch
  --hide-details-card          Start with the conversation environment and Git card closed
  --wide-canvas                Switch the Office split view to wide canvas mode
  --window-size <WxH>          Initial window size in logical px (default: 1100x780, min: 400x360)
  --light                      Launch in Light theme
  --dark                       Launch in unified Dark theme (KUI + Kantor)
  --smoke-integration          Launch the Stage-1 KUI + Kantor same-window smoke view
  --screenshot <PNG_PATH>      Render native Metal window to PNG after delay and exit cleanly
  --screenshot-delay-ms <MS>   Delay before capturing --screenshot (default: 1200)
  -h, --help                   Print this help message
";

struct CliOptions {
    ordinary_launch: bool,
    workbench: WorkbenchConfig,
    smoke_integration: bool,
    coedit_steps: Vec<&'static str>,
    coedit_resolve: Option<UserResolutionChoice>,
    bind_selection: bool,
    new_chat: bool,
    switch_tab_after_bind: Option<usize>,
    select_runtime: Option<String>,
    select_cell: Option<String>,
    serve_http: Option<String>,
    guidance_demo: bool,
    open_cmd_palette: bool,
    open_provider_health: bool,
    open_settings_tab: Option<SettingsTab>,
    open_files_tab: bool,
    browser_mobile: bool,
    browser_console: bool,
    browser_inspect: bool,
    browser_pin_element: bool,
    expand_git_diff: bool,
    enable_worktree_mode: bool,
    open_runtime_popover: bool,
    open_dropzone: bool,
    brief_rev: Option<u64>,
    collapse_sidebar: bool,
    wide_canvas: bool,
    window_size: Option<(f32, f32)>,
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
    let mut new_chat = false;
    let mut switch_tab_after_bind = None;
    let mut select_runtime = None;
    let mut select_cell = None;
    let mut serve_http = None;
    let mut guidance_demo = false;
    let mut open_cmd_palette = false;
    let mut open_provider_health = false;
    let mut open_settings_tab = None;
    let mut open_files_tab = false;
    let mut browser_mobile = false;
    let mut browser_console = false;
    let mut browser_inspect = false;
    let mut browser_pin_element = false;
    let mut expand_git_diff = false;
    let mut enable_worktree_mode = false;
    let mut open_runtime_popover = false;
    let mut open_dropzone = false;
    let mut brief_rev = None;
    let mut collapse_sidebar = false;
    let mut wide_canvas = false;
    let mut window_size = None;
    let mut screenshot_out = None;
    let mut screenshot_delay_ms = 1200u64;

    let mut ordinary_launch = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        // Storage, appearance and window size do not request a particular destination.
        ordinary_launch &= matches!(arg.as_str(), "--chat-dir" | "--workspace" | "--docs-dir"
            | "--lang" | "--light" | "--dark" | "--window-size") || arg.starts_with("-psn_");
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
            "--brief-demo" => {
                wb.start_brief_demo = true;
            }
            "--guidance-demo" => {
                wb.start_demo_chat = true;
                guidance_demo = true;
            }
            "--open-remote-sheet" => {
                wb.open_remote_sheet = true;
            }
            "--open-schedules-sheet" => {
                wb.open_schedules_sheet = true;
            }
            "--open-context-popover" => {
                wb.open_context_popover = true;
            }
            "--open-git-tab" => {
                wb.open_git_tab = true;
            }
            "--open-tasks-inbox" => {
                wb.open_tasks_inbox = true;
            }
            "--open-office-gallery" => {
                wb.open_office_gallery = true;
            }
            "--open-git-graph" => {
                wb.open_git_tab = true;
                wb.open_git_graph = true;
            }
            "--expand-git-diff" => {
                wb.open_git_tab = true;
                expand_git_diff = true;
            }
            "--open-files-tab" => {
                open_files_tab = true;
            }
            "--open-browser" => {
                wb.open_browser_tab = true;
            }
            "--browser-url" => {
                wb.open_browser_tab = true;
                wb.initial_browser_url = args.next();
            }
            "--browser-mobile" => {
                wb.open_browser_tab = true;
                browser_mobile = true;
            }
            "--browser-console" => {
                wb.open_browser_tab = true;
                browser_console = true;
            }
            "--browser-inspect" => {
                wb.open_browser_tab = true;
                browser_inspect = true;
            }
            "--browser-pin-element" => {
                wb.open_browser_tab = true;
                browser_pin_element = true;
            }
            "--open-terminal-tab" => {
                wb.open_terminal_tab = true;
            }
            "--open-bottom-terminal" => {
                wb.open_bottom_terminal = true;
            }
            "--hide-details-card" => {
                wb.hide_details_card = true;
            }
            "--lang" => {
                if let Some(v) = args.next() {
                    wb.lang_zh = v.eq_ignore_ascii_case("zh") || v.eq_ignore_ascii_case("chinese");
                    wb.lang_en = v.eq_ignore_ascii_case("en") || v.eq_ignore_ascii_case("english");
                }
            }
            "--run-terminal" => {
                if !wb.open_bottom_terminal {
                    wb.open_terminal_tab = true;
                }
                wb.initial_terminal_cmd = args.next();
            }
            "--enable-worktree-mode" => {
                enable_worktree_mode = true;
            }
            "--open-settings" => {
                if let Some(v) = args.next() {
                    open_settings_tab = Some(match v.as_str() {
                        "appearance" | "theme" => SettingsTab::Appearance,
                        "keybindings" | "keys" => SettingsTab::Keybindings,
                        "skills" | "mcp" => SettingsTab::SkillsMcp,
                        "usage" | "cache" => SettingsTab::Usage,
                        "projects" | "git" => SettingsTab::ProjectsGit,
                        "devices" | "preview" => SettingsTab::Devices,
                        "schedules" | "cron" => SettingsTab::Schedules,
                        "remote" | "pair" => SettingsTab::Remote,
                        "logs" | "diag" | "about" => SettingsTab::Logs,
                        "agents" | "providers" => SettingsTab::Agents,
                        "voice" => SettingsTab::Voice,
                        _ => SettingsTab::General,
                    });
                } else {
                    open_settings_tab = Some(SettingsTab::General);
                }
            }
            "--open-skills-sheet" => {
                wb.open_skills_sheet = true;
            }
            "--open-usage-sheet" => {
                wb.open_usage_sheet = true;
            }
            "--open-handoff-sheet" => {
                wb.open_handoff_sheet = true;
            }
            "--open-side-question" => {
                wb.open_side_question = true;
            }
            "--open-logs-sheet" => {
                wb.open_logs_sheet = true;
            }
            "--sidebar-files" => {
                wb.sidebar_repo_files = true;
            }
            "--open-dropzone" => {
                open_dropzone = true;
            }
            "--brief-rev" => {
                if let Some(v) = args.next() {
                    brief_rev = v.parse::<u64>().ok();
                }
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
            "--new-chat" => {
                new_chat = true;
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
            "--serve-http" => {
                serve_http = args.next();
            }
            "--open-cmd-palette" => {
                open_cmd_palette = true;
            }
            "--open-provider-health" => {
                open_provider_health = true;
            }
            "--open-runtime-popover" => {
                open_runtime_popover = true;
            }
            "--open-latest-chat" => {
                wb.open_latest_chat = true;
            }
            "--collapse-sidebar" => {
                collapse_sidebar = true;
            }
            "--wide-canvas" => {
                wide_canvas = true;
            }
            "--window-size" => {
                if let Some(v) = args.next()
                    && let Some((w, h)) = v.split_once('x').or_else(|| v.split_once('X'))
                    && let (Ok(wf), Ok(hf)) = (w.trim().parse::<f32>(), h.trim().parse::<f32>())
                {
                    window_size = Some((wf.max(400.), hf.max(360.)));
                }
            }
            "--dark" => {
                wb.dark_mode = true;
                wb.light_mode = false;
            }
            "--light" => {
                wb.light_mode = true;
                wb.dark_mode = false;
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
        ordinary_launch,
        workbench: wb,
        smoke_integration,
        coedit_steps,
        coedit_resolve,
        bind_selection,
        new_chat,
        switch_tab_after_bind,
        select_runtime,
        select_cell,
        serve_http,
        guidance_demo,
        open_cmd_palette,
        open_provider_health,
        open_settings_tab,
        open_files_tab,
        browser_mobile,
        browser_console,
        browser_inspect,
        browser_pin_element,
        expand_git_diff,
        enable_worktree_mode,
        open_runtime_popover,
        open_dropzone,
        brief_rev,
        collapse_sidebar,
        wide_canvas,
        window_size,
        screenshot_out,
        screenshot_delay_ms,
    }
}

fn main() {
    if std::env::args().any(|a| a == "--mcp-stdio-bridge") {
        kar_desktop::mcp_office::run_stdio_bridge();
    }
    kar_desktop::hardware::use_bundled_speech_engine();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,kar_desktop=info"),
    )
    .init();

    let opts = parse_cli();

    let app = gpui_kit::application().with_assets(kantor_ui::KantorAssets);
    app.run(move |cx| {
        init_runtime_ui(cx);

        let (init_w, init_h) = opts.window_size.unwrap_or((1100., 780.));
        let interactive = opts.screenshot_out.is_none();
        let mut window_options = studio_window_options(
            APP_NAME,
            size(px(init_w), px(init_h)),
            size(px(400.), px(360.)),
            cx,
        );

        window_options.focus = interactive;

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
            if interactive { cx.activate(true); }
            schedule_screenshot(
                window,
                None,
                None,
                opts.screenshot_out,
                opts.screenshot_delay_ms,
                cx,
            );
            return;
        }

        let mut wb_cfg = opts.workbench.clone();
        let new_chat_command = if opts.new_chat {
            wb_cfg.open_latest_chat = false;
            wb_cfg.resume_last_chat = false;
            wb_cfg.initial_terminal_cmd.take()
        } else {
            None
        };
        let (window, entity) = match gpui_kit::open_window(window_options, cx, move |window, cx| {
            cx.new(|cx| if opts.ordinary_launch {
                KarWorkbench::new_primary(wb_cfg, window, cx)
            } else {
                KarWorkbench::new(wb_cfg, window, cx)
            })
        }) {
            Ok(o) => o,
            Err(err) => {
                eprintln!("kar-desktop: failed to open window: {err:#}");
                std::process::exit(1);
            }
        };
        let bridge_tx = entity.read(cx).bridge.tx.clone();
        if interactive { cx.activate(true); }

        // Optional scripted UI steps (used for deterministic verification & screenshot capture)
        let steps = opts.coedit_steps.clone();
        let resolve = opts.coedit_resolve.clone();
        let bind_sel = opts.bind_selection;
        let create_chat = opts.new_chat;
        let switch_tab = opts.switch_tab_after_bind;
        let sel_rt = opts.select_runtime.clone();
        let sel_cell = opts.select_cell.clone();
        let srv_http = opts.serve_http.clone();
        let seed_guidance = opts.guidance_demo;
        let open_cmd = opts.open_cmd_palette;
        let open_prov = opts.open_provider_health;
        let open_set_tab = opts.open_settings_tab;
        let open_files = opts.open_files_tab;
        let br_mobile = opts.browser_mobile;
        let br_console = opts.browser_console;
        let br_inspect = opts.browser_inspect;
        let br_pin = opts.browser_pin_element;
        let exp_git_diff = opts.expand_git_diff;
        let enable_wt = opts.enable_worktree_mode;
        let open_rt_pop = opts.open_runtime_popover;
        let expand_dropzone = opts.open_dropzone;
        let target_brief_rev = opts.brief_rev;
        let collapse_rail = opts.collapse_sidebar;
        let wide_doc = opts.wide_canvas;
        if !steps.is_empty()
            || resolve.is_some()
            || bind_sel
            || create_chat
            || switch_tab.is_some()
            || sel_rt.is_some()
            || sel_cell.is_some()
            || srv_http.is_some()
            || seed_guidance
            || open_cmd
            || open_prov
            || open_set_tab.is_some()
            || open_files
            || br_mobile
            || br_console
            || br_inspect
            || br_pin
            || exp_git_diff
            || enable_wt
            || open_rt_pop
            || expand_dropzone
            || target_brief_rev.is_some()
            || collapse_rail
            || wide_doc
        {
            let win_handle = window;
            let wb_entity = entity.clone();
            let step_delay_ms = (opts.screenshot_delay_ms.saturating_sub(700)).clamp(850, 2400);
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(step_delay_ms))
                    .await;
                let _ = win_handle.update(cx, |_, window, cx| {
                    wb_entity.update(cx, |wb, cx| {
                        if seed_guidance {
                            wb.seed_guidance_demo(cx);
                        }
                        if let Some(ref rt_id) = sel_rt {
                            wb.select_runtime_by_id(rt_id, cx);
                        }
                        if enable_wt {
                            wb.new_chat_use_worktree = true;
                            cx.notify();
                        }
                        if create_chat && wb.create_new_real_chat(cx) {
                            wb.config.initial_terminal_cmd = new_chat_command;
                        }
                        if let Some(ref addr) = srv_http
                            && let Some(cid) = wb.chat_list.first().map(|c| c.info.id)
                        {
                            wb.bridge.send(KarCommand::StartHttpServer {
                                chat_id: cid,
                                listen_addr: addr.clone(),
                            });
                        }
                        if let Some(ref a1) = sel_cell {
                            wb.select_excel_cell_on_active_tab(a1, window, cx);
                        }
                        if bind_sel {
                            if switch_tab.is_some() && !wb.office_tabs.is_empty() {
                                wb.active_office_tab = Some(0);
                            }
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
                        if collapse_rail {
                            wb.set_sidebar_open(false, cx);
                        }
                        if wide_doc {
                            wb.set_layout_mode(WorkspaceLayoutMode::CanvasWide, cx);
                        }
                        if open_cmd {
                            wb.toggle_command_palette(window, cx);
                        }
                        if open_prov {
                            wb.toggle_provider_sheet(cx);
                        }
                        if let Some(tab) = open_set_tab {
                            wb.open_settings(tab, cx);
                        }
                        if open_files {
                            wb.open_files_dock_tab(cx);
                        }
                        if exp_git_diff {
                            wb.expand_first_git_diff(cx);
                        }
                        if open_rt_pop {
                            wb.open_menu = Some(Menu::ComposerAgent);
                            cx.notify();
                        }
                        if expand_dropzone {
                            wb.start_dropzone_open = true;
                            cx.notify();
                        }
                        if let Some(rev) = target_brief_rev {
                            wb.select_brief_revision(rev, cx);
                        }
                        if br_mobile {
                            if let Some(tab) = wb.browser.active_tab_mut() {
                                tab.viewport = kar_desktop::ui_browser::BrowserViewportPreset::Mobile;
                            }
                            cx.notify();
                        }
                        if br_console {
                            wb.browser.console_open = true;
                            cx.notify();
                        }
                        if br_inspect {
                            wb.toggle_browser_inspect_mode(cx);
                        }
                        if br_pin {
                            let url = wb
                                .browser
                                .active_tab()
                                .map_or_else(String::new, |t| t.url.clone());
                            let title = wb
                                .browser
                                .active_tab()
                                .map_or_else(String::new, |t| t.title.clone());
                            wb.pinned_target = Some(kar_desktop::types::SelectionTarget {
                                tab_id: 1,
                                file_path: None,
                                doc_name: kar_desktop::ui_browser::compact_url_display(&url),
                                kind: kar_desktop::types::OfficeKind::Word,
                                base_revision: None,
                                locator: kar_desktop::types::SelectionLocator::WebElement {
                                    url,
                                    page_title: title,
                                    selector: "button#cta-deploy".into(),
                                    tag_name: "button#cta-deploy.primary-cta".into(),
                                    bounds_summary: "x=24, y=218, w=164px, h=40px".into(),
                                    computed_styles: "display: inline-flex; background: rgb(37, 99, 235); radius: 10px".into(),
                                    html_snippet: "<button id=\"cta-deploy\" class=\"primary-cta\">立即发布 v0.1.0</button>".into(),
                                },
                                excerpt: "立即发布 v0.1.0".into(),
                            });
                            cx.notify();
                        }
                    });
                });
            })
            .detach();
        }

        schedule_screenshot(
            window,
            Some(entity),
            Some(bridge_tx),
            opts.screenshot_out,
            opts.screenshot_delay_ms,
            cx,
        );
    });
}

fn schedule_screenshot(
    window: AnyWindowHandle,
    workbench: Option<Entity<KarWorkbench>>,
    shutdown_tx: Option<async_channel::Sender<KarCommand>>,
    screenshot_out: Option<PathBuf>,
    delay_ms: u64,
    cx: &mut App,
) {
    #[cfg(target_os = "macos")]
    if let Some(out) = screenshot_out {
        cx.spawn(async move |cx| {
            let settle_ms = 520u64.min(delay_ms / 2);
            let warmup_ms = delay_ms.saturating_sub(settle_ms);
            cx.background_executor()
                .timer(Duration::from_millis(warmup_ms))
                .await;
            // Warm up one offscreen Metal pass so async SVG glyphs populate the atlas,
            // analytic springs (`Slide`, `pro_presence`, `pro_pill_indicator`) settle cleanly,
            // and any live WKWebView captures a fresh Retina PNG snapshot at its settled bounds.
            let _ = window.update(cx, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                if let Some(ref wb) = workbench {
                    wb.read(cx).refresh_active_browser_snapshot();
                }
                let _ = window.render_to_image();
            });
            cx.background_executor()
                .timer(Duration::from_millis(settle_ms))
                .await;
            let _ = window.update(cx, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
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
            });
            if let Some(tx) = shutdown_tx {
                let _ = tx.send(KarCommand::Shutdown).await;
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, workbench, shutdown_tx, screenshot_out, delay_ms, cx);
}
