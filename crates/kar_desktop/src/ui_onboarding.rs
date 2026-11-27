//! The first-run route belongs to the primary window. An interlude replay never changes the chat.
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_app_artwork, pro_icon};

use crate::{
    client_store::{desktop_preference, write_desktop_preference},
    onboarding_audio::IntroAudio,
    ui_motion::{Tactile, render_toast},
    ui_onboarding_film::{FILM_DURATION, film},
    ui_primitives::{StudioPalette, ui_font},
    workbench::{KarWorkbench, WorkbenchConfig},
};

const CHECKPOINT: &str = "onboarding-v1";
const REDUCED_DURATION: Duration = Duration::from_millis(1200);

#[derive(Default)]
pub(crate) enum Onboarding {
    #[default]
    Hidden,
    Welcome,
    Film {
        start: Instant,
        replay: bool,
        reduced: bool,
        _audio: Option<IntroAudio>,
    },
    Providers {
        reveal: Option<Instant>,
    },
}

fn checkpoint(dir: &Path, value: &str) {
    if let Err(err) = write_desktop_preference(dir, CHECKPOINT, value) {
        log::warn!("Could not save first-run progress: {err}");
    }
}

fn first_screen(dir: &Path) -> Onboarding {
    match desktop_preference(dir, CHECKPOINT).as_deref() {
        Some("welcome") => return Onboarding::Welcome,
        Some("connect") => return Onboarding::Providers { reveal: None },
        Some(_) => return Onboarding::Hidden,
        None => {}
    }

    // Check before the store or project registry is seeded by the workbench.
    let fresh = match std::fs::read_dir(dir) {
        Err(err) => err.kind() == io::ErrorKind::NotFound,
        Ok(mut entries) => !entries.any(|entry| {
            entry.map_or(true, |entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".karchat")
                    || matches!(
                        name.as_ref(),
                        "last-chat"
                            | "agent-choice"
                            | "appearance"
                            | "language"
                            | "desktop_preferences.json"
                            | "kar_desktop_projects.json"
                            | "keybindings.json"
                            | "sidebar-width"
                            | "split-ratio"
                    )
            })
        }),
    };
    if fresh {
        checkpoint(dir, "welcome");
        Onboarding::Welcome
    } else {
        Onboarding::Hidden
    }
}

impl KarWorkbench {
    /// Used for an ordinary desktop launch, as opposed to an explicit document or workbench route.
    pub fn new_primary(
        config: WorkbenchConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let screen = first_screen(&config.chat_dir);
        let mut wb = Self::new(config, window, cx);
        if !matches!(screen, Onboarding::Hidden) {
            wb.onboarding = screen;
            wb.focus_handle.focus(window, cx);
        }
        wb
    }

    pub(crate) fn play_intro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let replay = self.settings_open && matches!(self.onboarding, Onboarding::Hidden);
        if !replay && !matches!(self.onboarding, Onboarding::Welcome) {
            return;
        }
        if !replay {
            checkpoint(&self.config.chat_dir, "connect");
        }
        let reduced = cx.reduce_motion();
        let start = Instant::now();
        self.onboarding_provider = None;
        self.onboarding = Onboarding::Film {
            start,
            replay,
            reduced,
            _audio: (!reduced).then(IntroAudio::start),
        };
        let duration = if reduced {
            REDUCED_DURATION
        } else {
            FILM_DURATION
        };
        // macOS may suspend frame callbacks for a covered or minimized window.
        cx.spawn_in(window, async move |weak, cx| {
            cx.background_executor().timer(duration).await;
            let _ = weak.update_in(cx, |wb, window, cx| {
                if matches!(wb.onboarding, Onboarding::Film { start: current, .. } if current == start) {
                    wb.end_intro(window, cx);
                }
            });
        }).detach();
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn end_intro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Onboarding::Film {
            start,
            reduced,
            replay,
            ..
        } = self.onboarding
        else {
            return;
        };
        self.onboarding = if replay {
            Onboarding::Hidden
        } else {
            Onboarding::Providers {
                reveal: (!reduced && start.elapsed() >= FILM_DURATION).then(Instant::now),
            }
        };
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    pub(crate) fn skip_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.onboarding, Onboarding::Film { .. }) {
            self.end_intro(window, cx);
            return;
        }
        if matches!(self.onboarding, Onboarding::Hidden) {
            return;
        }
        checkpoint(&self.config.chat_dir, "done");
        self.onboarding_provider = None;
        self.provider_sign_in.dismiss();
        self.onboarding = Onboarding::Hidden;
        // The input handler is mounted when the welcome screen gives way to the workbench.
        cx.on_next_frame(window, |wb, window, cx| {
            if !wb.settings_open && matches!(wb.onboarding, Onboarding::Hidden) {
                wb.composer_editor
                    .update(cx, |editor, cx| editor.focus(window, cx));
            }
        });
        cx.notify();
    }

    pub(crate) fn complete_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.onboarding, Onboarding::Providers { .. }) {
            self.skip_onboarding(window, cx);
            self.start_new_chat(window, cx);
        }
    }

    pub(crate) fn onboarding_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if matches!(self.onboarding, Onboarding::Hidden) {
            return None;
        }
        if matches!(self.onboarding, Onboarding::Providers { .. })
            && self.finish_ready_onboarding_login(window, cx)
        {
            return None;
        }
        let s = StudioPalette::get(cx);
        let loc = self.ui_locale;
        let view = match &self.onboarding {
            Onboarding::Hidden => return None,
            Onboarding::Welcome => {
                let sign_in = loc.pick("登录", "Sign in");
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px(px(24.))
                    .child(
                        div()
                            .size(px(86.))
                            .rounded_full()
                            .border_1()
                            .border_color(s.line)
                            .flex()
                            .justify_center()
                            .items_center()
                            .when_some(pro_app_artwork("kense", 42., cx), |d, mark| d.child(mark)),
                    )
                    .child(
                        div()
                            .mt(px(20.))
                            .text_size(px(40.))
                            .line_height(px(48.))
                            .text_color(s.ink)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(crate::APP_NAME),
                    )
                    .child(
                        div()
                            .mt(px(6.))
                            .text_size(s.font_size(15.))
                            .text_color(s.muted)
                            .child(loc.pick(
                                "连接智能体，开始第一次对话。",
                                "Connect an agent to start your first conversation.",
                            )),
                    )
                    .child(
                        Tactile::ghost("onboarding-login", &s)
                            .mt(px(30.))
                            .w(px(188.))
                            .h(px(42.))
                            .radius(21.)
                            .justify_center()
                            .gap(px(8.))
                            .fills(s.ink, s.text)
                            .text_color(s.surface)
                            .text_size(s.font_size(15.))
                            .font_weight(FontWeight::MEDIUM)
                            .aria(sign_in)
                            .child(sign_in)
                            .child(pro_icon(ProIcon::ArrowRight, 16., s.surface))
                            .on_press(cx, |wb, window, cx| wb.play_intro(window, cx)),
                    )
                    .child(
                        Tactile::ghost("onboarding-skip-welcome", &s)
                            .mt(px(8.))
                            .h(px(34.))
                            .px(px(14.))
                            .radius(17.)
                            .text_color(s.muted)
                            .text_size(s.font_size(13.))
                            .aria(loc.pick("先进入工作台", "Continue without signing in"))
                            .child(loc.pick("先进入工作台", "Continue without signing in"))
                            .on_press(cx, |wb, window, cx| wb.skip_onboarding(window, cx)),
                    )
                    .into_any_element()
            }
            Onboarding::Film { start, reduced, .. } => {
                if !reduced {
                    window.request_animation_frame();
                }
                let skip = loc.pick("跳过", "Skip");
                div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(film(start.elapsed(), *reduced, window, cx))
                    .child(
                        div().absolute().top(px(16.)).right(px(20.)).child(
                            Tactile::ghost("onboarding-skip-film", &s)
                                .h(px(30.))
                                .px(px(12.))
                                .radius(15.)
                                .gap(px(6.))
                                .fills(rgba(0xffffffd9).into(), white())
                                .text_color(rgb(0x373b42))
                                .text_size(px(13.))
                                .aria(skip)
                                .child(skip)
                                .child(pro_icon(ProIcon::ChevronRight, 13., rgb(0x373b42).into()))
                                .on_press(cx, |wb, window, cx| wb.end_intro(window, cx)),
                        ),
                    )
                    .into_any_element()
            }
            Onboarding::Providers { reveal } => {
                let fade = reveal
                    .map(|time| time.elapsed().as_secs_f32() / 0.18)
                    .filter(|t| *t < 1.);
                if self.toast.as_ref().is_some_and(|t| t.expired()) {
                    self.toast = None;
                }
                div()
                    .relative()
                    .size_full()
                    .child(self.render_onboarding_providers(window, cx))
                    .when_some(fade, |picker, t| {
                        window.request_animation_frame();
                        picker.child(
                            div()
                                .absolute()
                                .inset_0()
                                .opacity(1. - t * t * (3. - 2. * t))
                                .child(film(FILM_DURATION, true, window, cx)),
                        )
                    })
                    .child(render_toast(self.toast.as_ref(), &s, window, cx))
                    .into_any_element()
            }
        };
        Some(
            div()
                .id("onboarding")
                .size_full()
                .relative()
                .bg(s.stage)
                .font_family(ui_font(cx))
                .text_color(s.ink)
                .track_focus(&self.focus_handle)
                .capture_key_down(cx.listener(|wb, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" && !ev.keystroke.modifiers.modified() {
                        wb.skip_onboarding(window, cx);
                    } else if ev.keystroke.key == "enter"
                        && wb.focus_handle.is_focused(window)
                        && matches!(wb.onboarding, Onboarding::Welcome)
                    {
                        wb.play_intro(window, cx);
                    } else {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                }))
                .child(view)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{CHECKPOINT, Onboarding, checkpoint, first_screen};

    #[test]
    fn fresh_profile_resumes_its_checkpoint_and_existing_chats_bypass_welcome() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("chats");
        assert!(matches!(first_screen(&dir), Onboarding::Welcome));
        assert!(matches!(first_screen(&dir), Onboarding::Welcome));
        checkpoint(&dir, "connect");
        assert!(matches!(first_screen(&dir), Onboarding::Providers { .. }));
        checkpoint(&dir, "done");
        assert!(matches!(first_screen(&dir), Onboarding::Hidden));

        for name in [
            "old.karchat",
            "desktop_preferences.json",
            "agent-choice",
            "kar_desktop_projects.json",
        ] {
            let dir = temp.path().join(name.replace('.', "_"));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join(name), "").unwrap();
            assert!(matches!(first_screen(&dir), Onboarding::Hidden));
            assert!(!dir.join(CHECKPOINT).exists());
        }
    }
}
