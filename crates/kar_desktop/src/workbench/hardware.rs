use super::*;
use crate::hardware::{Audio, Command, DeviceAction, Key, Outcome, Recording};
use crate::prefs::VoiceProviderKind;

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
    pub(crate) fn discard_voice_dictation(&mut self, cx: &mut Context<Self>) {
        self.held_voice_key = None;
        self.voice_owner = None;
        self.active_voice_rec = None;
        cx.notify();
    }
    pub fn toggle_voice_dictation(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(recording) = self.active_voice_rec.take() {
            self.held_voice_key = None;
            let owner = self.voice_owner.take().unwrap_or_else(|| {
                DraftLocation::for_mode(&self.access_mode, &self.config.workspace_dir)
            });
            match recording.stop() {
                Ok(samples) => {
                    let id = Uuid::new_v4();
                    self.hardware_owners.insert(id, owner);
                    self.bridge.send(KarCommand::Hardware {
                        workspace: self.effective_workspace_dir(),
                        request_id: id,
                        cmd: Command::Transcribe(
                            id,
                            Audio {
                                samples,
                                provider: self.prefs.voice_provider,
                                language: self.prefs.voice_language.clone(),
                                remove_fillers: self.prefs.voice_remove_fillers,
                                words: self.prefs.voice_custom_words.clone(),
                                model: self.prefs.voice_openrouter_model.clone(),
                            },
                        ),
                    });
                    self.notify_toast(self.ui_locale.pick("正在转写语音…", "Transcribing…"), cx);
                }
                Err(e) => self.notify_warning(e.to_string(), cx),
            }
        } else {
            match Recording::start() {
                Ok(rec) => {
                    self.voice_owner = Some(DraftLocation::for_mode(
                        &self.access_mode,
                        &self.config.workspace_dir,
                    ));
                    self.active_voice_rec = Some(rec);
                    if self.held_voice_key.is_some() {
                        self.notify_toast(
                            self.ui_locale.pick(
                                "录音中，松开快捷键以转写",
                                "Recording. Release the shortcut to transcribe.",
                            ),
                            cx,
                        );
                    }
                }
                Err(e) => self.notify_warning(e.to_string(), cx),
            }
        }
        cx.notify();
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
        match result {
            Ok(Outcome::Transcript(_, text)) if text.trim().is_empty() => self.notify_warning(
                self.ui_locale.pick("未识别到语音", "No speech detected"),
                cx,
            ),
            Ok(Outcome::Transcript(_, text)) => {
                if let Some(owner) = owner {
                    if owner
                        == DraftLocation::for_mode(&self.access_mode, &self.config.workspace_dir)
                    {
                        self.composer_editor.update(cx, |e, cx| {
                            let current = e.value();
                            let combined = if current.trim().is_empty() {
                                text
                            } else {
                                format!("{current} {text}")
                            };
                            e.set_value(combined, window, cx);
                            e.focus(window, cx);
                        });
                    } else {
                        self.prepend_composer_draft(
                            owner,
                            QueuedPrompt {
                                text,
                                ..Default::default()
                            },
                            window,
                            cx,
                        );
                    }
                    self.notify_toast(
                        self.ui_locale
                            .pick("听写已加入草稿", "Dictation added to draft"),
                        cx,
                    );
                }
            }
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
