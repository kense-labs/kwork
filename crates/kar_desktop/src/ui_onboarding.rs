//! The first-run route belongs to the primary window. An interlude replay never changes the chat.
use std::{
    f32::consts::TAU,
    io,
    path::Path,
    time::{Duration, Instant},
};

use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_app_artwork, pro_icon};

use crate::{
    client_store::{desktop_preference, write_desktop_preference},
    onboarding_audio::{IntroAudio, UiSfx},
    ui_motion::{Tactile, render_toast},
    ui_onboarding_film::{FILM_DURATION, film, welcome_ground},
    ui_primitives::{
        HEADER_H, StudioPalette, mono_font, render_provider_color_tile, titlebar_row, ui_font,
    },
    workbench::{KarWorkbench, WorkbenchConfig},
};

const CHECKPOINT: &str = "onboarding-v1";
const REDUCED_DURATION: Duration = Duration::from_millis(1200);

#[derive(Default)]
pub(crate) enum Onboarding {
    #[default]
    Hidden,
    Welcome {
        audio: Option<IntroAudio>,
    },
    Film {
        start: Instant,
        replay: bool,
        reduced: bool,
        audio: Option<IntroAudio>,
    },
    Providers {
        reveal: Option<Instant>,
        audio: Option<IntroAudio>,
    },
}

impl Onboarding {
    fn take_audio(&mut self) -> Option<IntroAudio> {
        match self {
            Self::Hidden => None,
            Self::Welcome { audio } | Self::Film { audio, .. } | Self::Providers { audio, .. } => {
                audio.take()
            }
        }
    }

    pub(crate) fn audio(&self) -> Option<&IntroAudio> {
        match self {
            Self::Hidden => None,
            Self::Welcome { audio } | Self::Film { audio, .. } | Self::Providers { audio, .. } => {
                audio.as_ref()
            }
        }
    }
}

fn checkpoint(dir: &Path, value: &str) {
    if let Err(err) = write_desktop_preference(dir, CHECKPOINT, value) {
        log::warn!("Could not save first-run progress: {err}");
    }
}

fn first_screen(dir: &Path) -> Onboarding {
    match desktop_preference(dir, CHECKPOINT).as_deref() {
        Some("welcome") => return Onboarding::Welcome { audio: None },
        Some("connect") => {
            return Onboarding::Providers {
                reveal: None,
                audio: None,
            };
        }
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
        Onboarding::Welcome { audio: None }
    } else {
        Onboarding::Hidden
    }
}

fn paint_medallion_ring(bounds: Bounds<Pixels>, s: &StudioPalette, window: &mut Window) {
    let center = bounds.center();
    let outer_r = f32::from(bounds.size.width) * 0.47;
    let inner_r = 47.;

    let line_tone = if s.dark {
        rgb(0x60a5fa).opacity(0.16)
    } else {
        rgb(0x3b82f6).opacity(0.14)
    };
    let accent_tone = if s.dark {
        rgb(0x38bdf8).opacity(0.36)
    } else {
        rgb(0x2563eb).opacity(0.28)
    };

    let mut ring = PathBuilder::stroke(px(1.0));
    for k in 0..4 {
        let start_a = k as f32 * (TAU * 0.25) + 0.18;
        let span = TAU * 0.25 - 0.36;
        ring.move_to(center + point(px(outer_r * start_a.cos()), px(outer_r * start_a.sin())));
        for step in 1..=12 {
            let a = start_a + span * (step as f32 / 12.);
            ring.line_to(center + point(px(outer_r * a.cos()), px(outer_r * a.sin())));
        }
        let ca = k as f32 * (TAU * 0.25);
        ring.move_to(center + point(px((inner_r + 5.) * ca.cos()), px((inner_r + 5.) * ca.sin())));
        ring.line_to(center + point(px((outer_r - 5.) * ca.cos()), px((outer_r - 5.) * ca.sin())));
    }
    if let Ok(path) = ring.build() {
        window.paint_path(path, line_tone);
    }

    let mut gems = PathBuilder::fill();
    for k in 0..4 {
        let a = k as f32 * (TAU * 0.25);
        let p = center + point(px(outer_r * a.cos()), px(outer_r * a.sin()));
        let d = 2.8;
        gems.move_to(p + point(px(0.), px(-d)));
        gems.line_to(p + point(px(d), px(0.)));
        gems.line_to(p + point(px(0.), px(d)));
        gems.line_to(p + point(px(-d), px(0.)));
        gems.close();
    }
    if let Ok(path) = gems.build() {
        window.paint_path(path, accent_tone);
    }
}

impl KarWorkbench {
    /// Used for an ordinary desktop launch, as opposed to an explicit document or workbench route.
    pub fn new_primary(
        config: WorkbenchConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut screen = first_screen(&config.chat_dir);
        let mut wb = Self::new(config, window, cx);
        if !matches!(screen, Onboarding::Hidden) {
            if !cx.reduce_motion() {
                match &mut screen {
                    Onboarding::Welcome { audio } | Onboarding::Providers { audio, .. } => {
                        *audio = Some(IntroAudio::prewarm());
                    }
                    Onboarding::Film { .. } | Onboarding::Hidden => {}
                }
            }
            wb.onboarding = screen;
            wb.focus_handle.focus(window, cx);
        }
        wb
    }

    pub(crate) fn play_onboarding_sfx(&self, sfx: UiSfx, cx: &App) {
        if !cx.reduce_motion()
            && let Some(audio) = self.onboarding.audio()
        {
            audio.play_sfx(sfx);
        }
    }

    pub(crate) fn play_intro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let replay = self.settings_open && matches!(self.onboarding, Onboarding::Hidden);
        if !replay && !matches!(self.onboarding, Onboarding::Welcome { .. }) {
            return;
        }
        if !replay {
            checkpoint(&self.config.chat_dir, "connect");
        }
        let reduced = cx.reduce_motion();
        let start = Instant::now();
        let mut audio = self.onboarding.take_audio();
        if reduced {
            audio = None;
        } else if let Some(warm) = &audio {
            warm.play_film();
        } else {
            audio = Some(IntroAudio::start_film());
        }
        self.onboarding_provider = None;
        self.onboarding = Onboarding::Film {
            start,
            replay,
            reduced,
            audio,
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
        })
        .detach();
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
        let elapsed = start.elapsed();
        let skipped_early = !reduced && elapsed + Duration::from_millis(80) < FILM_DURATION;
        let audio = self.onboarding.take_audio();
        if let Some(ref stream) = audio
            && skipped_early
        {
            stream.stop_film();
            stream.play_sfx(UiSfx::Dismiss);
        }
        self.onboarding = if replay {
            Onboarding::Hidden
        } else {
            Onboarding::Providers {
                reveal: (!reduced && !skipped_early).then(Instant::now),
                audio,
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
        self.play_onboarding_sfx(UiSfx::Dismiss, cx);
        self.finish_onboarding_to_workbench(window, cx);
    }

    fn finish_onboarding_to_workbench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            self.play_onboarding_sfx(UiSfx::Connected, cx);
            self.finish_onboarding_to_workbench(window, cx);
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
            Onboarding::Welcome { .. } => {
                let sign_in = loc.pick("登录", "Sign in");
                let pal = s;
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px(px(24.))
                    .child(
                        div()
                            .relative()
                            .size(px(140.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, _| {
                                        paint_medallion_ring(bounds, &pal, window);
                                    },
                                )
                                .absolute()
                                .inset_0(),
                            )
                            .child(
                                div()
                                    .size(px(90.))
                                    .rounded_full()
                                    .bg(s.card)
                                    .border_1()
                                    .border_color(s.line_strong)
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .when_some(pro_app_artwork("kense", 42., cx), |d, mark| {
                                        d.child(mark)
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .font_family(mono_font(cx))
                            .text_size(px(10.5))
                            .text_color(s.muted)
                            .child(div().size(px(5.)).rounded_full().bg(rgb(0x10b981)))
                            .child("KENSE AGENT ONLINE // STANDBY"),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(42.))
                            .line_height(px(50.))
                            .text_color(s.ink)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(crate::APP_NAME),
                    )
                    .child(
                        div()
                            .mt(px(6.))
                            .text_size(s.font_size(15.))
                            .text_color(s.muted)
                            .text_center()
                            .child(loc.pick(
                                "连接智能体，开始第一次对话。",
                                "Connect an agent to start your first conversation.",
                            )),
                    )
                    .child(
                        Tactile::ghost("onboarding-login", &s)
                            .mt(px(30.))
                            .w(px(200.))
                            .h(px(44.))
                            .radius(22.)
                            .elevated(true)
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
                let elapsed = start.elapsed();
                if !reduced {
                    window.request_animation_frame();
                }
                let skip = loc.pick("跳过", "Skip");
                let dark_stage = !*reduced && elapsed.as_secs_f32() < 3.55;
                let (pill_rest, pill_hover, pill_ink) = if dark_stage {
                    (
                        rgba(0x0f172ab8).into(),
                        rgba(0x1e293bd8).into(),
                        rgb(0xe2e8f0).into(),
                    )
                } else {
                    (rgba(0xffffffdc).into(), white(), rgb(0x334155).into())
                };
                // Pre-populate GPUI's SVG atlas for installed providers during the final 0.7s
                // of the film so the "Choose an agent" list renders icons on its very first frame.
                let warm_icons = elapsed.as_secs_f32() > 5.4 || *reduced;
                let mut prewarm = div().absolute().left(px(-200.)).top(px(-200.)).opacity(0.);
                if warm_icons {
                    for p in self
                        .providers
                        .iter()
                        .filter(|p| !crate::aster::is_aster(&p.id))
                    {
                        prewarm = prewarm.child(render_provider_color_tile(&p.id, 30., &s, cx));
                    }
                }
                div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .when(warm_icons, |d| d.child(prewarm))
                    .child(film(elapsed, *reduced, window, cx))
                    .child(
                        div().absolute().top(px(14.)).right(px(18.)).child(
                            Tactile::ghost("onboarding-skip-film", &s)
                                .h(px(30.))
                                .px(px(12.))
                                .radius(15.)
                                .gap(px(5.))
                                .fills(pill_rest, pill_hover)
                                .text_color(pill_ink)
                                .text_size(px(12.5))
                                .aria(skip)
                                .child(skip)
                                .child(pro_icon(ProIcon::ChevronRight, 13., pill_ink))
                                .on_press(cx, |wb, window, cx| wb.end_intro(window, cx)),
                        ),
                    )
                    .into_any_element()
            }
            Onboarding::Providers { reveal, .. } => {
                let fade = reveal
                    .map(|time| time.elapsed().as_secs_f32() / 0.32)
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
                        let veil = 1. - t * t * (3. - 2. * t);
                        picker.child(div().absolute().inset_0().bg(welcome_ground(veil)))
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
                        && matches!(wb.onboarding, Onboarding::Welcome { .. })
                    {
                        wb.play_intro(window, cx);
                    } else {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                }))
                .child(view)
                .child(
                    // Keep the top frameless window strip draggable without blocking the top-right Skip button.
                    titlebar_row(window)
                        .absolute()
                        .top_0()
                        .left_0()
                        .right(px(110.))
                        .h(px(HEADER_H.min(44.))),
                )
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
        assert!(matches!(first_screen(&dir), Onboarding::Welcome { .. }));
        assert!(matches!(first_screen(&dir), Onboarding::Welcome { .. }));
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
