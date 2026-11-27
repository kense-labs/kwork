//! One local Whisper session per recording. Only the unfinished phrase is revised.
use super::{Capture, RATE, VoiceState, supported_speech_model};
use anyhow::{Context, Result, ensure};
use async_channel::{Receiver, Sender};
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

// Start with enough audio for a short word, then send the freshest available
// window as soon as the preceding decode returns.
const STEP: usize = RATE * 2 / 5;
const WINDOW: usize = 12 * RATE;
const HOP: usize = RATE / 50;

pub(super) struct LiveDictation {
    updates: Sender<String>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Drop for LiveDictation {
    fn drop(&mut self) {
        self.updates.close();
        if let Ok(mut guard) = self.child.lock()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.kill();
        }
    }
}

impl LiveDictation {
    pub(super) fn start(
        capture: Arc<Mutex<Capture>>,
        state: &VoiceState,
        language: &str,
        words: &[String],
    ) -> Option<(Self, Receiver<String>)> {
        let program = preview_program()?;
        let model = preview_model(state, &program, language)?;
        let mut command = Command::new(program);
        command.arg("--preview").arg("--model").arg(model);
        if !language.is_empty() && language != "auto" {
            command.arg("--language").arg(language);
        }
        let vocabulary = words
            .iter()
            .map(|word| {
                word.split_once("=>")
                    .map_or(word.as_str(), |(_, term)| term.trim())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let (updates, rx) = async_channel::bounded(1);
        let child = Arc::new(Mutex::new(None));
        let live = Self {
            updates: updates.clone(),
            child: child.clone(),
        };
        thread::Builder::new()
            .name("dictation-preview".into())
            .spawn(move || {
                if let Err(error) = recognize(command, vocabulary, capture, &updates, &child)
                    && !updates.is_closed()
                {
                    log::warn!("Local speech preview ended: {error:#}");
                }
                updates.close();
                let finished = child.lock().unwrap().take();
                if let Some(mut child) = finished {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            })
            .ok()?;
        Some((live, rx))
    }
}

fn preview_program() -> Option<PathBuf> {
    let name = format!("kwrok-dictation{}", std::env::consts::EXE_SUFFIX);
    if let Some(program) = std::env::var_os("KAR_VOICE_ENGINE").map(PathBuf::from)
        && program.file_name()? == name.as_str()
    {
        return Some(program);
    }
    #[cfg(target_os = "macos")]
    if let Some(contents) = super::bundle_contents() {
        return Some(contents.join("Helpers").join(name));
    }
    Some(std::env::current_exe().ok()?.with_file_name(name))
}

fn preview_model(state: &VoiceState, program: &std::path::Path, language: &str) -> Option<PathBuf> {
    let linked = state
        .models
        .iter()
        .find(|m| {
            m["active"] == true
                && m["state"] == "installed"
                && m["id"].as_str().is_some_and(supported_speech_model)
                && (matches!(language, "" | "auto")
                    || m["languages"]
                        .as_array()
                        .is_some_and(|langs| langs.iter().any(|lang| lang == language)))
        })
        .and_then(|m| m["linkedFrom"].as_str())
        .map(PathBuf::from);
    if let Some(path) = linked.filter(|p| p.is_file()) {
        return Some(path);
    }
    #[cfg(target_os = "macos")]
    if let Some(path) = super::bundled_model() {
        return Some(path);
    }
    let sibling = program
        .parent()?
        .parent()?
        .join("Resources/Dictation/whisper-base-Q8_0.gguf");
    sibling.is_file().then_some(sibling)
}

fn recognize(
    mut command: Command,
    vocabulary: String,
    capture: Arc<Mutex<Capture>>,
    updates: &Sender<String>,
    child: &Mutex<Option<Child>>,
) -> Result<()> {
    // Vocabulary is local to this invocation; audio travels only over these two pipes.
    let mut terms = None;
    if !vocabulary.is_empty() {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(vocabulary.as_bytes())?;
        command.arg("--vocabulary-file").arg(file.path());
        terms = Some(file);
    }
    let (mut input, mut output) = {
        let mut guard = child.lock().unwrap();
        if updates.is_closed() {
            return Ok(());
        }
        // Leave capacity for rendering while the recognizer repeatedly encodes the current phrase.
        if std::env::var_os("RAYON_NUM_THREADS").is_none() {
            let threads = thread::available_parallelism().map_or(1, |n| n.get().div_ceil(2).min(4));
            command.env("RAYON_NUM_THREADS", threads.to_string());
        }
        let mut process = command
            .env_remove("KAR_WORKBENCH_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = process.stdin.take().context("missing dictation stdin")?;
        let output = BufReader::new(process.stdout.take().context("missing dictation stdout")?);
        *guard = Some(process);
        (input, output)
    };
    let mut start = 0;
    let mut seen = 0;
    let mut prefix = String::new();
    let mut last_sent = String::new();
    while !updates.is_closed() {
        let window = {
            let data = capture
                .lock()
                .map_err(|_| anyhow::anyhow!("recording closed"))?;
            if data.error.is_some() {
                break;
            }
            if data.samples.len().saturating_sub(seen) < STEP {
                drop(data);
                thread::sleep(Duration::from_millis(40));
                continue;
            }
            let available = &data.samples[start..];
            let threshold = speech_threshold(&available[..available.len().min(WINDOW)]);
            let (length, commit) = boundary(available, threshold);
            let samples = &available[..length];
            let newly_voiced = has_speech(
                &samples[seen.saturating_sub(start).min(length)..],
                threshold,
            );
            seen = start + length;
            if !newly_voiced && !commit {
                continue;
            }
            let window = samples.to_vec();
            if commit {
                start = seen;
            }
            (window, commit, threshold)
        };
        let (samples, commit, threshold) = window;
        if !has_speech(&samples, threshold) {
            continue;
        }
        // Read the response before considering another window, coalescing intervening callbacks.
        let mut frame = Vec::with_capacity(4 + 4 * samples.len());
        frame.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        for value in samples {
            frame.extend_from_slice(&value.to_le_bytes());
        }
        input.write_all(&frame)?;
        input.flush()?;
        let mut line = String::new();
        ensure!(output.read_line(&mut line)? > 0, "dictation helper closed");
        let text: String = serde_json::from_str(&line)?;
        if text.trim().is_empty() {
            // A partial word can temporarily receive an empty hypothesis.
            if commit {
                prefix.clone_from(&last_sent);
            }
            continue;
        }
        let mut draft = prefix.clone();
        if draft.ends_with(|c: char| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '?' | '!' | ',' | ':' | ';')
        }) && text.trim().starts_with(|c: char| c.is_ascii_alphanumeric())
        {
            draft.push(' ');
        }
        draft.push_str(text.trim());
        if draft != last_sent {
            if updates.force_send(draft.clone()).is_err() {
                break;
            }
            last_sent.clone_from(&draft);
        }
        if commit {
            prefix = draft;
        }
    }
    drop(terms);
    Ok(())
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32
}

fn speech_threshold(samples: &[f32]) -> f32 {
    // Estimate the microphone's noise floor from recent 60 ms spans. A quiet
    // speaker can have clear syllables even when every sample is below -42 dBFS.
    let recent = &samples[samples.len().saturating_sub(2 * RATE)..];
    let floor = recent
        .chunks_exact(3 * HOP)
        .map(energy)
        .fold(f32::INFINITY, f32::min);
    (floor * 6.25).clamp(0.0009_f32.powi(2), 0.008_f32.powi(2))
}

fn has_speech(samples: &[f32], threshold: f32) -> bool {
    samples
        .chunks_exact(HOP)
        .filter(|chunk| energy(chunk) >= threshold)
        .take(3)
        .count()
        == 3
}

fn boundary(samples: &[f32], threshold: f32) -> (usize, bool) {
    if samples.len() >= WINDOW {
        // Carry the exact boundary into the next window. Prefer a quiet spot near its end.
        let search = WINDOW - RATE;
        let quietest = samples[search..WINDOW]
            .chunks_exact(HOP)
            .enumerate()
            .min_by(|(_, a), (_, b)| energy(a).total_cmp(&energy(b)))
            .map_or(0, |(i, _)| i);
        return (search + HOP * quietest + HOP / 2, true);
    }
    let pause =
        samples.len() >= 4 * RATE && !has_speech(&samples[samples.len() - RATE / 2..], threshold);
    (samples.len(), pause)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_keep_short_clicks_and_quiet_room_tone_out_of_previews() {
        let mut samples = vec![0.002; RATE];
        samples[0] = 1.;
        assert!(!has_speech(&samples, speech_threshold(&samples)));
        samples[HOP..4 * HOP].fill(0.02);
        assert!(has_speech(&samples, speech_threshold(&samples)));
    }

    #[test]
    fn quiet_speech_above_room_tone_can_start_a_preview() {
        let mut samples = vec![0.001; STEP];
        assert!(!has_speech(&samples, speech_threshold(&samples)));
        samples[3 * HOP..8 * HOP].fill(0.004);
        assert!(has_speech(&samples, speech_threshold(&samples)));
    }

    #[test]
    #[ignore = "requires the Rust speech helper, a Whisper model and a spoken Mandarin 16 kHz f32 fixture"]
    fn quiet_microphone_emits_revisions_before_recording_stops() -> Result<()> {
        let model = std::env::var("KWORK_TEST_VOICE_MODEL")?;
        let bytes = std::fs::read(std::env::var("KWORK_TEST_ZH_UTTERANCE")?)?;
        let mut samples: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let loudest = samples
            .chunks_exact(HOP)
            .map(energy)
            .fold(0., f32::max)
            .sqrt();
        ensure!(loudest > 0., "the test fixture must contain speech");
        for sample in &mut samples {
            *sample *= 0.006 / loudest;
        }
        let capture = Arc::new(Mutex::new(Capture::default()));
        let state = VoiceState {
            models: vec![serde_json::json!({
                "id": super::super::BUNDLED_MODEL, "active": true, "state": "installed",
                "languages": ["zh"], "linkedFrom": model
            })],
            ..Default::default()
        };
        let (_live, updates) = LiveDictation::start(capture.clone(), &state, "auto", &[])
            .context("missing speech preview helper or model")?;
        let start = std::time::Instant::now();
        let mut elapsed = Duration::ZERO;
        let mut revisions = Vec::new();
        for callback in samples.chunks(HOP) {
            elapsed += Duration::from_secs_f64(callback.len() as f64 / RATE as f64);
            thread::sleep(elapsed.saturating_sub(start.elapsed()));
            capture.lock().unwrap().push(callback, 1, RATE);
            if let Ok(text) = updates.try_recv() {
                eprintln!("preview at {:.2}s: {text}", start.elapsed().as_secs_f32());
                revisions.push(text);
            }
        }
        ensure!(
            revisions.len() >= 2,
            "expected successive previews while speaking: {revisions:?}"
        );
        ensure!(
            revisions
                .iter()
                .any(|text| text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))),
            "expected Chinese speech: {revisions:?}"
        );
        Ok(())
    }

    #[test]
    fn previews_revise_one_phrase_until_a_pause_or_bounded_window() {
        let mut samples = vec![0.05; WINDOW + RATE];
        let threshold = speech_threshold(&samples);
        assert_eq!(boundary(&samples[..2 * RATE], threshold), (2 * RATE, false));
        samples[4 * RATE..5 * RATE].fill(0.);
        assert_eq!(boundary(&samples[..5 * RATE], threshold), (5 * RATE, true));
        let quiet = WINDOW - 2 * HOP..WINDOW - HOP;
        samples[quiet.clone()].fill(0.);
        let (cut, commit) = boundary(&samples, threshold);
        assert!(commit && quiet.contains(&cut));
    }
}
