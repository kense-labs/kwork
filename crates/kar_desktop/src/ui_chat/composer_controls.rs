//! Transient composer controls for attached files and microphone capture.
use super::*;
use crate::hardware::VOICE_METER_BARS;
use kui::primitives::{family::disabled_motion, glyph, ops_surface::Fold};

struct RecordingMeter {
    workbench: WeakEntity<KarWorkbench>,
    levels: [f32; VOICE_METER_BARS],
}

impl Render for RecordingMeter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = StudioPalette::get(cx);
        if disabled_motion(cx) {
            return pro_icon(ProIcon::Mic, ICON_SM, s.pro.red_ink);
        }
        if let Some(workbench) = self.workbench.upgrade()
            && let Some(recording) = &workbench.read(cx).active_voice_rec
        {
            if let Some(levels) = recording.waveform() {
                self.levels = levels;
            }
            window.request_animation_frame();
        }
        div()
            .h(px(20.))
            .flex()
            .items_center()
            .gap(px(2.))
            .children(self.levels.iter().map(|level| {
                div()
                    .w(px(2.))
                    .h(px((3. + 15. * level).round()))
                    .rounded_full()
                    .bg(s.pro.red_ink)
            }))
            .into_any_element()
    }
}

impl KarWorkbench {
    pub(super) fn render_recording_controls(
        &self,
        width: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let workbench = cx.weak_entity();
        let meter = window.use_state(cx, |_, _| RecordingMeter {
            workbench,
            levels: [0.; VOICE_METER_BARS],
        });
        let finish = loc.pick("完成听写", "Finish dictation");
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(div().px(px(5.)).child(meter))
            .when(width >= 440., |row| {
                row.child(
                    div()
                        .mr(px(4.))
                        .text_size(s.font_size(FS_META))
                        .text_color(s.muted)
                        .child(loc.pick("听写中", "Listening")),
                )
            })
            .child(
                icon_button_sm(
                    "cmp-discard-voice",
                    ProIcon::Close,
                    loc.pick("放弃这段录音", "Discard recording"),
                    s,
                )
                .on_press(cx, |this, window, cx| {
                    this.discard_voice_dictation(cx);
                    this.composer_editor.read(cx).focus.clone().focus(window, cx);
                }),
            )
            .child(
                Tactile::ghost("cmp-confirm-voice", s)
                    .size(px(CTRL_SM_H))
                    .flex_none()
                    .face_inset(2.)
                    .radius(R_PILL)
                    .fills(s.ink, s.text)
                    .justify_center()
                    .aria(finish)
                    .tip(finish)
                    .child(glyph::icon(Glyph::Check, ICON, s.card))
                    .on_press(cx, |this, win, cx| {
                        if this.active_voice_rec.is_some() {
                            this.toggle_voice_dictation(win, cx);
                            this.composer_editor.read(cx).focus.clone().focus(win, cx);
                        }
                    }),
            )
    }

    pub(super) fn render_attachment_tray(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let owner = self
            .current_chat_id()
            .map(ElementId::Uuid)
            .unwrap_or_else(|| ElementId::Path(self.config.workspace_dir.as_path().into()));
        window.with_id(owner, |window| {
            let open = !self.pending_attachments.is_empty();
            let fold = Fold::get("composer-attachment-tray", open, 34., 0.2, window, cx);
            if !fold.mounted() {
                return div();
            }
            // Retain the last row until collapse finishes; a closed chip cannot remove a new file.
            let retained = window.use_state(cx, |_, _| Vec::<PathBuf>::new());
            if open && *retained.read(cx) != self.pending_attachments {
                retained.update(cx, |paths, _| paths.clone_from(&self.pending_attachments));
            }
            let mut row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .pb(px(6.));
            for path in retained.read(cx).clone() {
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                let file_mark = OfficeKind::for_path(&path)
                    .map(|kind| render_office_chip(kind, s).into_any_element())
                    .unwrap_or_else(|| pro_icon(ProIcon::PageDoc, ICON_SM, s.muted));
                let key = ElementId::Path(path.as_path().into());
                let remove = format!(
                    "{} {name}",
                    self.ui_locale.pick("移除附件", "Remove attachment")
                );
                let chip = div()
                    .h(px(28.))
                    .max_w_full()
                    .min_w_0()
                    .pl(px(8.))
                    .pr(px(3.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .border_1()
                    .border_color(s.line_soft)
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(file_mark)
                    .child(
                        div()
                            .min_w_0()
                            .max_w(px(160.))
                            .truncate()
                            .text_size(s.font_size(FS_META))
                            .text_color(s.ink)
                            .child(name),
                    )
                    .child(
                        close_button(key.clone(), remove, s)
                            .disabled(!open)
                            .on_press(cx, move |this, _, cx| {
                                this.pending_attachments.retain(|item| item != &path);
                                cx.notify();
                            }),
                    );
                row = row.child(arrive(chip, key, 0, 2., cx));
            }
            fold.wrap(
                0.,
                div().px(px(16.)).child(
                    div()
                        .id("pending-attachment-list")
                        .w_full()
                        .max_h(px(68.))
                        .overflow_y_scroll()
                        .child(row),
                ),
            )
        })
    }
}
