//! Native microphone capture and the desktop-facing contracts for Kar voice and device routes.
use crate::{engine::Engine, prefs::VoiceProviderKind};
use anyhow::{Context, Result, anyhow, bail};
use axum::http::Method;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

const RATE: usize = 16_000;
const MAX_FRAMES: usize = RATE * 300;

pub struct Recording {
    stream: cpal::Stream,
    audio: Arc<Mutex<Capture>>,
}
#[derive(Default)]
struct Capture {
    samples: Vec<f32>,
    next: usize,
    sum: f32,
    frames: usize,
    error: Option<String>,
}
impl Capture {
    fn push<T: cpal::Sample>(&mut self, data: &[T], channels: usize, input_rate: usize)
    where
        f32: cpal::FromSample<T>,
    {
        // Keep the resampling phase across callbacks, and mix stereo inputs to one channel.
        for frame in data.chunks_exact(channels) {
            if self.samples.len() == MAX_FRAMES {
                return;
            }
            let mono = frame.iter().map(|&v| v.to_sample::<f32>()).sum::<f32>() / channels as f32;
            self.sum += mono;
            self.frames += 1;
            self.next += RATE;
            while self.next >= input_rate {
                self.next -= input_rate;
                if self.samples.len() == MAX_FRAMES {
                    return;
                }
                self.samples.push(
                    if self.frames == 0 {
                        mono
                    } else {
                        self.sum / self.frames as f32
                    }
                    .clamp(-1., 1.),
                );
                self.sum = 0.;
                self.frames = 0;
            }
        }
    }
}
impl Recording {
    pub fn microphone() -> Option<String> {
        cpal::default_host()
            .default_input_device()
            .and_then(|d| d.name().ok())
    }
    pub fn start() -> Result<Self> {
        let device = cpal::default_host()
            .default_input_device()
            .context("没有可用的输入设备")?;
        let format = device
            .default_input_config()
            .context("读取麦克风格式失败")?;
        let config = format.config();
        let audio = Arc::new(Mutex::new(Capture::default()));
        fn stream<T: cpal::SizedSample>(
            d: &cpal::Device,
            c: &cpal::StreamConfig,
            audio: Arc<Mutex<Capture>>,
        ) -> Result<cpal::Stream>
        where
            f32: cpal::FromSample<T>,
        {
            let errors = audio.clone();
            let channels = c.channels as usize;
            let rate = c.sample_rate.0 as usize;
            anyhow::ensure!(channels > 0 && rate > 0, "无效的麦克风采样率");
            Ok(d.build_input_stream(
                c,
                move |data: &[T], _| {
                    if let Ok(mut capture) = audio.lock()
                        && capture.error.is_none()
                    {
                        capture.push(data, channels, rate);
                    }
                },
                move |e| {
                    if let Ok(mut capture) = errors.lock() {
                        capture.error = Some(e.to_string())
                    }
                },
                None,
            )?)
        }
        let stream = match format.sample_format() {
            cpal::SampleFormat::F32 => stream::<f32>(&device, &config, audio.clone())?,
            cpal::SampleFormat::I16 => stream::<i16>(&device, &config, audio.clone())?,
            cpal::SampleFormat::U16 => stream::<u16>(&device, &config, audio.clone())?,
            cpal::SampleFormat::I32 => stream::<i32>(&device, &config, audio.clone())?,
            cpal::SampleFormat::F64 => stream::<f64>(&device, &config, audio.clone())?,
            other => bail!("不支持的麦克风格式 {other}"),
        };
        stream.play().context("无法启动麦克风")?;
        Ok(Self { stream, audio })
    }
    pub fn stop(self) -> Result<Vec<f32>> {
        drop(self.stream);
        let mut capture = self.audio.lock().map_err(|_| anyhow!("麦克风采集失败"))?;
        if let Some(err) = capture.error.take() {
            bail!(err);
        }
        anyhow::ensure!(!capture.samples.is_empty(), "未收到麦克风音频");
        Ok(std::mem::take(&mut capture.samples))
    }
}

pub struct Audio {
    pub samples: Vec<f32>,
    pub provider: VoiceProviderKind,
    pub language: String,
    pub remove_fillers: bool,
    pub words: Vec<String>,
    pub model: Option<String>,
}
impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("frames", &self.samples.len())
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}
impl Audio {
    fn packet(&self) -> Result<Vec<u8>> {
        anyhow::ensure!(
            !self.samples.is_empty() && self.samples.len() <= MAX_FRAMES,
            "无效的录音长度"
        );
        let mut words: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for entry in &self.words {
            if let Some((from, to)) = entry.split_once("=>") {
                let aliases = words.entry(to.trim().into()).or_default();
                if !aliases.iter().any(|w| w == from.trim())
                    && !from.trim().eq_ignore_ascii_case(to.trim())
                {
                    aliases.push(from.trim().into());
                }
            } else {
                words.entry(entry.trim().into()).or_default();
            }
        }
        let words = words
            .into_iter()
            .map(|(term, aliases)| json!({"term":term,"aliases":aliases}))
            .collect::<Vec<_>>();
        let options = serde_json::to_vec(
            &json!({"speechLanguage":self.language,"speechTranslateToEnglish":false,"speechCustomWords":words,"speechRemoveFillerWords":self.remove_fillers}),
        )?;
        let mut packet = Vec::with_capacity(4 + options.len() + self.samples.len() * 4);
        packet.extend_from_slice(&(options.len() as u32).to_le_bytes());
        packet.extend_from_slice(&options);
        for sample in &self.samples {
            packet.extend_from_slice(&sample.to_le_bytes());
        }
        Ok(packet)
    }
}

pub struct Key(pub Option<String>);
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}
pub fn provider_id(p: VoiceProviderKind) -> &'static str {
    match p {
        VoiceProviderKind::Grok => "xai",
        other => other.key(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct VoiceState {
    pub status: Value,
    pub keys: BTreeMap<String, bool>,
    pub models: Vec<Value>,
    pub cloud_models: Vec<Value>,
    pub cloud_model_error: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub host_id: String,
    pub id: String,
    pub platform: String,
    pub name: String,
    pub version: String,
    pub booted: bool,
    pub physical: bool,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DeviceState {
    pub settings: Value,
    pub devices: Vec<Device>,
    pub hosts: Vec<Value>,
}

#[derive(Debug)]
pub enum DeviceAction {
    Configure(Value),
    AddSsh(Vec<Value>),
    Boot(Device, Uuid),
    Shutdown(Device),
    Preview(Device, u16),
    Screenshot(Device, PathBuf),
}
#[derive(Debug)]
pub enum Command {
    VoiceStatus(bool),
    VoiceKey(VoiceProviderKind, Key),
    VoiceModel(String, String),
    Transcribe(Uuid, Audio),
    Devices(Option<DeviceAction>),
}
#[derive(Clone, Debug)]
pub enum Outcome {
    Voice(VoiceState),
    KeySaved(VoiceProviderKind),
    Transcript(Uuid, String),
    TranscriptionFailed(Uuid, String),
    Devices(DeviceState),
    Screenshot(PathBuf),
}

fn target(device: &Device) -> Value {
    json!({"host_id":device.host_id,"device_id":device.id})
}
pub(crate) async fn run(engine: Engine, cmd: Command) -> Result<Outcome> {
    match cmd {
        Command::Transcribe(id, audio) => {
            let result = async {
                let data = audio.packet()?;
                let path = if audio.provider == VoiceProviderKind::Local {
                    "/api/voice/transcribe".into()
                } else {
                    let mut path = format!(
                        "/api/voice/cloud/{}/transcribe",
                        provider_id(audio.provider)
                    );
                    if audio.provider == VoiceProviderKind::OpenRouter {
                        let model = audio
                            .model
                            .as_deref()
                            .filter(|m| !m.is_empty())
                            .context("请先在语音设置中选择 OpenRouter 转写模型")?;
                        path.push('?');
                        path.push_str(
                            &form_urlencoded::Serializer::new(String::new())
                                .append_pair("model", model)
                                .finish(),
                        );
                    }
                    path
                };
                let reply = engine.post_audio(&path, data).await?;
                Ok::<_, anyhow::Error>(
                    reply["text"]
                        .as_str()
                        .context("Kar 未返回转写文本")?
                        .to_owned(),
                )
            }
            .await;
            Ok(match result {
                Ok(text) => Outcome::Transcript(id, text),
                Err(e) => Outcome::TranscriptionFailed(id, e.to_string()),
            })
        }
        Command::VoiceKey(provider, key) => {
            engine
                .request(
                    Method::POST,
                    &format!("/api/voice/cloud/{}/key", provider_id(provider)),
                    Some(json!({"key":key.0})),
                )
                .await?;
            Ok(Outcome::KeySaved(provider))
        }
        Command::VoiceModel(action, model) => {
            anyhow::ensure!(
                matches!(action.as_str(), "select" | "download"),
                "无效的模型操作"
            );
            engine
                .request(
                    Method::POST,
                    &format!("/api/voice/models/{action}"),
                    Some(json!({"modelId":model})),
                )
                .await?;
            voice(&engine, false).await
        }
        Command::VoiceStatus(cloud) => voice(&engine, cloud).await,
        Command::Devices(action) => {
            if let Some(action) = action {
                let (route, data) = match action {
                    DeviceAction::Configure(config) => ("/devices/configure", config),
                    DeviceAction::AddSsh(additions) => {
                        // The engine accepts a complete host list. Serialize desktop read-modify-write requests.
                        static UPDATE_HOSTS: tokio::sync::Mutex<()> =
                            tokio::sync::Mutex::const_new(());
                        let _writing = UPDATE_HOSTS.lock().await;
                        let snapshot = engine.request(Method::GET, "/devices", None).await?;
                        let mut hosts = snapshot["settings"]["ssh_hosts"]
                            .as_array()
                            .context("无法读取 SSH 主机")?
                            .clone();
                        let initial_len = hosts.len();
                        for host in additions {
                            if !hosts.iter().any(|known| same_ssh_endpoint(known, &host)) {
                                hosts.push(host);
                            }
                        }
                        if hosts.len() != initial_len {
                            engine
                                .request(
                                    Method::POST,
                                    "/devices/configure",
                                    Some(json!({"ssh_hosts":hosts})),
                                )
                                .await?;
                        }
                        return device_snapshot(&engine).await;
                    }
                    DeviceAction::Boot(device, chat) => {
                        let mut body = target(&device);
                        body["project"] = json!(engine.project_id());
                        body["chat"] = json!(chat);
                        body["platform"] = json!(device.platform);
                        body["boot"] = true.into();
                        ("/devices/open", body)
                    }
                    DeviceAction::Shutdown(device) => ("/devices/shutdown", target(&device)),
                    DeviceAction::Preview(device, port) => {
                        let mut body = target(&device);
                        body["type"] = "openUrl".into();
                        anyhow::ensure!(
                            device.host_id == "local" && !device.physical,
                            "仅本机模拟器可直接预览本地服务"
                        );
                        let host = if device.platform == "android" {
                            "10.0.2.2"
                        } else {
                            "localhost"
                        };
                        body["url"] = format!("http://{host}:{port}").into();
                        ("/devices/action", body)
                    }
                    DeviceAction::Screenshot(device, dir) => {
                        let query = form_urlencoded::Serializer::new(String::new())
                            .append_pair("host_id", &device.host_id)
                            .append_pair("device_id", &device.id)
                            .finish();
                        let img = engine
                            .read_binary(format!("/devices/screenshot?{query}"))
                            .await?;
                        anyhow::ensure!(
                            img.starts_with(b"\x89PNG\r\n\x1a\n"),
                            "设备未返回 PNG 截图"
                        );
                        let dir = dir.join("device_screenshots");
                        std::fs::create_dir_all(&dir)?;
                        let path = dir.join(format!("{}.png", Uuid::new_v4()));
                        std::fs::write(&path, img)?;
                        return Ok(Outcome::Screenshot(path));
                    }
                };
                engine.request(Method::POST, route, Some(data)).await?;
            }
            device_snapshot(&engine).await
        }
    }
}
pub(crate) fn same_ssh_endpoint(left: &Value, right: &Value) -> bool {
    left["target"].as_str().is_some()
        && left["target"] == right["target"]
        && left["port"].as_u64().unwrap_or(22) == right["port"].as_u64().unwrap_or(22)
}
async fn device_snapshot(engine: &Engine) -> Result<Outcome> {
    let snapshot = engine.request(Method::GET, "/devices", None).await?;
    Ok(Outcome::Devices(serde_json::from_value(snapshot)?))
}
async fn voice(engine: &Engine, cloud: bool) -> Result<Outcome> {
    let status = engine
        .request(Method::GET, "/api/voice/status", None)
        .await?;
    let keys = serde_json::from_value(
        engine
            .request(Method::GET, "/api/voice/cloud/keys", None)
            .await?,
    )?;
    let models = engine
        .request(Method::GET, "/api/voice/models", None)
        .await?["models"]
        .as_array()
        .context("无法读取语音模型目录")?
        .clone();
    let (cloud_models, cloud_model_error) = if cloud {
        match engine
            .request(Method::GET, "/api/voice/cloud/openrouter/models", None)
            .await
            .and_then(|v| {
                v["models"]
                    .as_array()
                    .cloned()
                    .context("无法读取 OpenRouter 模型")
            }) {
            Ok(models) => (models, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        }
    } else {
        (Vec::new(), None)
    };
    Ok(Outcome::Voice(VoiceState {
        status,
        keys,
        models,
        cloud_models,
        cloud_model_error,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_host() -> Result<(tempfile::TempDir, Engine)> {
        let dir = tempfile::tempdir()?;
        let project = dir.path().join("workspace");
        std::fs::create_dir(&project)?;
        let host = Engine::open(
            kar::chat::ChatStore::open(dir.path().join("chats"))?,
            &project,
        )?;
        Ok((dir, host))
    }
    fn audio(samples: Vec<f32>) -> Audio {
        Audio {
            samples,
            provider: VoiceProviderKind::Local,
            language: "en".into(),
            words: vec!["native g p u => GPUI".into()],
            remove_fillers: true,
            model: None,
        }
    }
    #[test]
    fn capture_preserves_sample_phase_across_stereo_callbacks() {
        let mut capture = Capture::default();
        capture.push(&[0.0, 1.0, 0.5, 0.5], 2, 48000);
        assert!(capture.samples.is_empty());
        capture.push(&[0.2, 0.8], 2, 48000);
        assert!((capture.samples[0] - 0.5).abs() < 1e-6);
        let data = audio(capture.samples).packet().unwrap();
        let len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        let options: Value = serde_json::from_slice(&data[4..4 + len]).unwrap();
        assert_eq!(
            options["speechCustomWords"],
            json!([{"term":"GPUI","aliases":["native g p u"]}])
        );
        assert_eq!(data.len(), 4 + len + 4);
    }
    #[tokio::test]
    async fn voice_credentials_and_device_settings_roundtrip_through_kar() -> Result<()> {
        let (_dir, host) = test_host()?;
        let Outcome::Devices(default) = run(host.clone(), Command::Devices(None)).await? else {
            unreachable!()
        };
        assert_eq!(default.settings["enabled"], false);
        assert!(default.devices.is_empty());
        let Outcome::Devices(enabled) = run(
            host.clone(),
            Command::Devices(Some(DeviceAction::Configure(json!({"enabled":true})))),
        )
        .await?
        else {
            unreachable!()
        };
        assert_eq!(enabled.settings["enabled"], true);
        assert!(enabled.hosts.iter().any(|h| h["id"] == "local"));
        run(
            host.clone(),
            Command::Devices(Some(DeviceAction::Configure(json!({"enabled":false})))),
        )
        .await?;
        let provider = VoiceProviderKind::OpenAi;
        let Outcome::Voice(initial) = run(host.clone(), Command::VoiceStatus(false)).await? else {
            unreachable!()
        };
        assert!(!initial.keys["openai"]);
        assert!(!initial.models.is_empty());
        let id = Uuid::new_v4();
        let mut missing = audio(vec![0.; 1600]);
        missing.provider = provider;
        let Outcome::TranscriptionFailed(request, err) =
            run(host.clone(), Command::Transcribe(id, missing)).await?
        else {
            unreachable!()
        };
        assert_eq!(request, id);
        assert!(err.contains("missing-key"), "{err}");
        run(
            host.clone(),
            Command::VoiceKey(provider, Key(Some("test-speech-only".into()))),
        )
        .await?;
        let Outcome::Voice(updated) = run(host.clone(), Command::VoiceStatus(false)).await? else {
            unreachable!()
        };
        assert!(updated.keys["openai"]);
        run(host.clone(), Command::VoiceKey(provider, Key(None))).await?;
        let Outcome::Voice(cleared) = run(host, Command::VoiceStatus(false)).await? else {
            unreachable!()
        };
        assert!(!cleared.keys["openai"]);
        Ok(())
    }
    #[tokio::test]
    #[ignore = "requires transcribe-cli, the pinned whisper-tiny.en GGUF and a 16 kHz mono f32 fixture"]
    async fn local_speech_reaches_native_kar_inference() -> Result<()> {
        let (_dir, host) = test_host()?;
        let bytes = std::fs::read(std::env::var("KWORK_TEST_UTTERANCE")?)?;
        let samples = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| f32::from_le_bytes(*v))
            .collect();
        let model = std::env::var("KWORK_TEST_VOICE_MODEL")?;
        host.request(
            Method::POST,
            "/api/voice/models/link",
            Some(json!({"path":model})),
        )
        .await?;
        host.request(
            Method::POST,
            "/api/voice/models/select",
            Some(json!({"modelId":"handy-computer/whisper-tiny.en-gguf"})),
        )
        .await?;
        let Outcome::Transcript(_, text) =
            run(host, Command::Transcribe(Uuid::new_v4(), audio(samples))).await?
        else {
            bail!("real transcription failed")
        };
        assert!(text.to_lowercase().contains("country"), "{text}");
        eprintln!("Native Kar transcription: {text}");
        Ok(())
    }
}
