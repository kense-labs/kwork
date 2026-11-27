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

const STEP: usize = RATE;
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
        thread::sleep(Duration::from_millis(120));
        let window = {
            let data = capture
                .lock()
                .map_err(|_| anyhow::anyhow!("recording closed"))?;
            if data.error.is_some() {
                break;
            }
            if data.samples.len().saturating_sub(seen) < STEP {
                continue;
            }
            let available = &data.samples[start..];
            let (length, commit) = boundary(available);
            let samples = &available[..length];
            let newly_voiced = has_speech(&samples[seen.saturating_sub(start).min(length)..]);
            seen = start + length;
            if !newly_voiced && !commit {
                continue;
            }
            let window = samples.to_vec();
            if commit {
                start = seen;
            }
            (window, commit)
        };
        let (samples, commit) = window;
        if !has_speech(&samples) {
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

fn has_speech(samples: &[f32]) -> bool {
    samples
        .chunks_exact(HOP)
        .filter(|chunk| energy(chunk) >= 0.008 * 0.008)
        .take(3)
        .count()
        == 3
}

fn boundary(samples: &[f32]) -> (usize, bool) {
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
    let pause = samples.len() >= 4 * RATE && !has_speech(&samples[samples.len() - RATE / 2..]);
    (samples.len(), pause)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_keep_short_clicks_and_quiet_room_tone_out_of_previews() {
        let mut samples = vec![0.002; RATE];
        samples[0] = 1.;
        assert!(!has_speech(&samples));
        samples[HOP..4 * HOP].fill(0.02);
        assert!(has_speech(&samples));
    }

    #[test]
    fn previews_revise_one_phrase_until_a_pause_or_bounded_window() {
        let mut samples = vec![0.05; WINDOW + RATE];
        assert_eq!(boundary(&samples[..2 * RATE]), (2 * RATE, false));
        samples[4 * RATE..5 * RATE].fill(0.);
        assert_eq!(boundary(&samples[..5 * RATE]), (5 * RATE, true));
        let quiet = WINDOW - 2 * HOP..WINDOW - HOP;
        samples[quiet.clone()].fill(0.);
        let (cut, commit) = boundary(&samples);
        assert!(commit && quiet.contains(&cut));
    }
}
