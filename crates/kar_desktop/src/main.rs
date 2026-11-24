extern crate gpui_kit as gpui;

use std::path::PathBuf;
use std::time::Duration;

use gpui::*;
use kar_desktop::{SmokeIntegrationView, init_runtime_ui};

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,kar_desktop=info"),
    )
    .init();

    let mut sample_docx = None;
    let mut screenshot_out = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--screenshot" {
            screenshot_out = args.next().map(PathBuf::from);
        } else if arg.ends_with(".docx") {
            sample_docx = Some(PathBuf::from(arg));
        }
    }

    let app = gpui_kit::application().with_assets(kantor_ui::KantorAssets);
    app.run(move |cx| {
        init_runtime_ui(cx);

        let options = WindowOptions {
            app_id: Some("kar-desktop".into()),
            titlebar: Some(TitlebarOptions {
                title: Some("Kar Desktop · KUI + Kantor".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1360.), px(840.)),
                cx,
            ))),
            window_min_size: Some(size(px(860.), px(540.))),
            ..Default::default()
        };

        let sample = sample_docx.clone();
        let (window, _view) = match gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| SmokeIntegrationView::new(sample, window, cx))
        }) {
            Ok(opened) => opened,
            Err(err) => {
                eprintln!("kar-desktop: failed to open window: {err:#}");
                std::process::exit(1);
            }
        };
        cx.activate(true);

        #[cfg(target_os = "macos")]
        if let Some(out) = screenshot_out {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(650))
                    .await;
                let _ = window.update(cx, |_, window, cx| {
                    match window.render_to_image() {
                        Ok(img) => {
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
    });
}
