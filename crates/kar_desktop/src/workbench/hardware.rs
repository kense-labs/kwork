use super::*;
use crate::hardware::{Audio, Command, DeviceAction, Key, Outcome, Recording};
use crate::prefs::VoiceProviderKind;

pub(super) struct DictationDraft {
    owner: DraftLocation,
    preview: String,
    discarded: bool,
    workspace: PathBuf,
    options: Option<Audio>,
}

/// Separate Latin words from adjacent words, including mixed Chinese/English drafts.
pub(crate) fn speech_suffix(draft: &str, speech: &str) -> String {
    let speech = speech.trim();
    let joins_words = draft
        .chars()
        .next_back()
        .zip(speech.chars().next())
        .is_some_and(|(left, right)| {
            left.is_alphanumeric()
                && right.is_alphanumeric()
                && (left.is_ascii_alphanumeric() || right.is_ascii_alphanumeric())
        });
    if joins_words {
        format!(" {speech}")
    } else {
        speech.to_owned()
    }
}

impl KarWorkbench {
    pub(crate) fn hardware(&self, cmd: Command) -> Uuid {
        let request_id = Uuid::new_v4();
        self.bridge.send(KarCommand::Hardware {
            workspace: self.effective_workspace_dir(),
            request_id,
            cmd,
        });
        request_id
    }
    pub(crate) fn device_command(&mut self, action: Option<DeviceAction>) {
        let shot = matches!(action, Some(DeviceAction::Screenshot(..)));
        let id = self.hardware(Command::Devices(action));
        if shot {
            self.hardware_owners.insert(
                id,
                DraftLocation::for_mode(&self.access_mode, &self.config.workspace_dir),
            );
        }
    }
    pub(crate) fn refresh_voice(&self) {
        self.hardware(Command::VoiceStatus(
            self.prefs.voice_provider == VoiceProviderKind::OpenRouter,
        ));
    }
    pub(crate) fn composer_dictation(&self) -> Option<&str> {
        self.voice_drafts
            .values()
            .find(|draft| !draft.discarded && draft.owner == self.drafts.location)
            .map(|draft| draft.preview.as_str())
    }

    pub(crate) fn discard_voice_dictation(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.voice_capture_id.take() {
            self.voice_drafts.remove(&id);
        } else if let Some(draft) = self
            .voice_drafts
            .values_mut()
            .find(|draft| !draft.discarded && draft.owner == self.drafts.location)
        {
            // The final provider request may already be running. Ignore its response.
            draft.discarded = true;
            draft.preview.clear();
        }
        self.held_voice_key = None;
        self.active_voice_rec = None;
        cx.notify();
    }

    pub(super) fn finish_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.voice_capture_id.take() else {
            return;
        };
        self.held_voice_key = None;
        let Some(recording) = self.active_voice_rec.take() else {
            return;
        };
        match recording.stop() {
            Ok(samples) => {
                let draft = self.voice_drafts.get_mut(&id).expect("active dictation");
                let mut audio = draft.options.take().expect("active recording options");
                audio.samples = samples;
                self.bridge.send(KarCommand::Hardware {
                    workspace: draft.workspace.clone(),
                    request_id: id,
                    cmd: Command::Transcribe(id, audio),
                });
            }
            Err(error) => {
                if let Some(draft) = self.voice_drafts.remove(&id) {
                    self.commit_dictation(&draft.owner, &draft.preview, window, cx);
                }
                self.notify_warning(error.to_string(), cx);
            }
        }
        cx.notify();
    }

    pub fn toggle_voice_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_voice_rec.is_some() {
            self.finish_dictation(window, cx);
            return;
        }
        if self.composer_dictation().is_some() {
            return;
        }
        let mut recording = match Recording::start() {
            Ok(recording) => recording,
            Err(error) => {
                self.notify_warning(error.to_string(), cx);
                return;
            }
        };
        let options = Audio {
            samples: Vec::new(),
            provider: self.prefs.voice_provider,
            language: self.prefs.voice_language.clone(),
            remove_fillers: self.prefs.voice_remove_fillers,
            words: self.prefs.voice_custom_words.clone(),
            model: self.prefs.voice_openrouter_model.clone(),
        };
        let id = Uuid::new_v4();
        if let Some(updates) =
            recording.live_transcript(&self.voice_env, &options.language, &options.words)
        {
            cx.spawn(async move |workbench, cx| {
                while let Ok(mut text) = updates.recv().await {
                    while let Ok(latest) = updates.try_recv() {
                        text = latest;
                    }
                    let active = workbench.update(cx, |workbench, cx| {
                        if workbench.voice_capture_id != Some(id) {
                            return false;
                        }
                        if let Some(draft) = workbench.voice_drafts.get_mut(&id)
                            && draft.preview != text
                        {
                            draft.preview = text;
                            cx.notify();
                        }
                        true
                    });
                    if !matches!(active, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();
        }
        self.voice_drafts.insert(
            id,
            DictationDraft {
                owner: self.drafts.location.clone(),
                preview: String::new(),
                discarded: false,
                workspace: self.effective_workspace_dir(),
                options: Some(options),
            },
        );
        self.voice_capture_id = Some(id);
        self.active_voice_rec = Some(recording);
        cx.notify();
    }

    fn commit_dictation(
        &mut self,
        owner: &DraftLocation,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.trim().is_empty() {
            return;
        }
        if owner == &self.drafts.location {
            self.composer_editor.update(cx, |editor, cx| {
                let suffix = speech_suffix(editor.value(), text);
                editor.set_dictation_preview("", cx);
                editor.append_text(&suffix, window, cx);
            });
        } else {
            let draft = self.drafts.saved.entry(owner.clone()).or_default();
            draft.text.push_str(&speech_suffix(&draft.text, text));
        }
    }

    pub(super) fn on_hardware(
        &mut self,
        workspace: PathBuf,
        request: Uuid,
        result: Result<Outcome, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = self.hardware_owners.remove(&request);
        self.migrating_voice_keys.remove(&request);
        let mut retained_preview = false;
        if let Some(draft) = self.voice_drafts.remove(&request) {
            if draft.discarded {
                return;
            }
            let owns_editor = draft.owner == self.drafts.location;
            match &result {
                Ok(Outcome::Transcript(_, text)) if !text.trim().is_empty() => {
                    self.commit_dictation(&draft.owner, text, window, cx);
                }
                _ => {
                    // A cloud final or a disconnected device can fail after a useful
                    // local hypothesis. Keep the recognized words in the original draft.
                    retained_preview = !draft.preview.trim().is_empty();
                    self.commit_dictation(&draft.owner, &draft.preview, window, cx);
                }
            }
            // A late setup error from another chat must not open Settings over the current one.
            if !owns_editor && matches!(result, Ok(Outcome::VoiceSetupRequired(_))) {
                return;
            }
        }
        match result {
            Ok(Outcome::Transcript(_, text)) if text.trim().is_empty() => self.notify_warning(
                if retained_preview {
                    self.ui_locale
                        .pick("已保留录音时识别的文字", "Kept the live transcript")
                } else {
                    self.ui_locale.pick("未识别到语音", "No speech detected")
                },
                cx,
            ),
            Ok(Outcome::VoiceSetupRequired(issue)) => {
                self.open_settings(crate::ui_settings::SettingsTab::Voice, cx);
                self.notify_warning(issue.message(self.ui_locale), cx);
            }
            Ok(Outcome::TranscriptionFailed(_, err)) | Err(err) => self.notify_warning(err, cx),
            Ok(Outcome::Screenshot(path)) => {
                if let Some(owner) = owner {
                    self.prepend_composer_draft(
                        owner,
                        QueuedPrompt {
                            attachments: vec![path],
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    self.notify_toast(
                        self.ui_locale
                            .pick("设备截图已加入草稿", "Screenshot attached to draft"),
                        cx,
                    );
                }
            }
            Ok(Outcome::Devices(devices)) if workspace == self.effective_workspace_dir() => {
                if let Some(hosts) = devices.settings["ssh_hosts"].as_array() {
                    let before = self.prefs.device_ssh_hosts.len();
                    self.prefs.device_ssh_hosts.retain(|legacy| {
                        !hosts
                            .iter()
                            .any(|h| crate::hardware::same_ssh_endpoint(h, &legacy.backend_host()))
                    });
                    if before != self.prefs.device_ssh_hosts.len() {
                        self.save_prefs();
                    }
                }
                self.devices = devices;
            }
            Ok(Outcome::Voice(state)) => {
                self.voice_env = state;
                for provider in [
                    VoiceProviderKind::OpenAi,
                    VoiceProviderKind::Grok,
                    VoiceProviderKind::OpenRouter,
                ] {
                    let k = provider.key();
                    if let Some(secret) = self
                        .prefs
                        .voice_api_keys
                        .get(k)
                        .filter(|v| !v.is_empty())
                        .cloned()
                    {
                        if self
                            .voice_env
                            .keys
                            .get(crate::hardware::provider_id(provider))
                            == Some(&true)
                        {
                            self.prefs.voice_api_keys.remove(k);
                            self.save_prefs();
                        } else if !self.migrating_voice_keys.values().any(|key| key == k) {
                            let id = self.hardware(Command::VoiceKey(provider, Key(Some(secret))));
                            self.migrating_voice_keys.insert(id, k.into());
                        }
                    }
                }
            }
            Ok(Outcome::KeySaved(provider)) => {
                if self.prefs.voice_api_keys.remove(provider.key()).is_some() {
                    self.save_prefs();
                }
                self.refresh_voice();
                self.notify_toast(
                    self.ui_locale
                        .pick("语音凭据已更新", "Speech credential updated"),
                    cx,
                );
            }
            _ => {}
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::speech_suffix;

    #[test]
    fn dictation_joins_mixed_words_without_splitting_chinese_or_punctuation() {
        assert_eq!(speech_suffix("已有草稿", "And so"), " And so");
        assert_eq!(speech_suffix("Kwork", "可以听写"), " 可以听写");
        assert_eq!(speech_suffix("today", " is Friday "), " is Friday");
        assert_eq!(speech_suffix("今天是", "星期五"), "星期五");
        assert_eq!(speech_suffix("可以", "。"), "。");
        assert_eq!(speech_suffix("已有草稿，", "今天"), "今天");
        assert_eq!(speech_suffix("已有草稿 ", "And so"), "And so");
        assert_eq!(speech_suffix("", "正在听写"), "正在听写");
    }
}
