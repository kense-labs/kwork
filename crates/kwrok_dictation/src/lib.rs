//! Offline dictation from a self-contained Whisper GGUF model on a Rust CPU backend.
#![forbid(unsafe_code)]

mod audio;
mod model;

use anyhow::{Context, Result, anyhow, ensure};
use audio::Frontend;
use candle_core::{Device, IndexOp, Tensor};
use candle_transformers::models::whisper::{self, model::Whisper};
use tokenizers::Tokenizer;

const SECOND: usize = whisper::SAMPLE_RATE;
const WINDOW: usize = SECOND * 30;

pub struct Recognizer {
    model: Whisper,
    frontend: Frontend,
    tokenizer: Tokenizer,
    languages: Vec<(String, u32)>,
    begin_suppress: Vec<u32>,
    eot: u32,
    sot: u32,
    no_speech: Option<u32>,
}

impl Recognizer {
    fn new(
        model: Whisper,
        frontend: Frontend,
        tokenizer: Tokenizer,
        begin_suppress: Vec<u32>,
        languages: Vec<String>,
    ) -> Result<Self> {
        ensure!(
            tokenizer.get_vocab_size(false) == model.config.vocab_size,
            "Whisper tokenizer and embedding have different vocabulary sizes"
        );
        ensure!(
            model.config.max_target_positions >= 8,
            "invalid Whisper text context"
        );
        let token = |s: &str| {
            tokenizer
                .token_to_id(s)
                .with_context(|| format!("missing Whisper token {s}"))
        };
        let eot = token(whisper::EOT_TOKEN)?;
        let sot = token(whisper::SOT_TOKEN)?;
        let no_speech = whisper::NO_SPEECH_TOKENS
            .iter()
            .find_map(|s| tokenizer.token_to_id(s));
        let languages = languages
            .into_iter()
            .filter_map(|lang| {
                tokenizer
                    .token_to_id(&format!("<|{lang}|>"))
                    .map(|id| (lang, id))
            })
            .collect();
        Ok(Self {
            model,
            frontend,
            tokenizer,
            languages,
            begin_suppress,
            eot,
            sot,
            no_speech,
        })
    }

    pub fn transcribe(
        &mut self,
        pcm: &[f32],
        language: Option<&str>,
        translate: bool,
        vocabulary: &str,
    ) -> Result<String> {
        ensure!(
            pcm.iter().all(|s| s.is_finite()),
            "audio contains invalid samples"
        );
        let language = language.filter(|s| !s.is_empty() && *s != "auto");
        let requested = language
            .map(|lang| {
                self.languages
                    .iter()
                    .find(|(s, _)| s == lang)
                    .map(|(_, id)| *id)
                    .with_context(|| format!("model does not support language {lang}"))
            })
            .transpose()?;
        let task = if translate {
            whisper::TRANSLATE_TOKEN
        } else {
            whisper::TRANSCRIBE_TOKEN
        };
        let prompt = self.vocabulary_prompt(vocabulary)?;
        ensure!(
            !translate || self.languages.len() > 1,
            "English-only Whisper models do not translate"
        );
        let mut output = String::new();
        let mut remaining = pcm;
        while !remaining.is_empty() {
            let cut = segment_end(remaining);
            let (segment, rest) = remaining.split_at(cut);
            remaining = rest;
            // The frontend also supplies a short silent WAV when probing model readiness.
            if segment.iter().all(|s| s.abs() <= 1e-6) {
                continue;
            }
            let text = self.decode_segment(segment, &prompt, requested, task)?;
            if output.ends_with(|c: char| c.is_ascii_alphanumeric())
                && text.starts_with(|c: char| c.is_ascii_alphanumeric())
            {
                output.push(' ');
            }
            output.push_str(&text);
        }
        Ok(output.trim().to_string())
    }

    fn vocabulary_prompt(&self, vocabulary: &str) -> Result<Vec<u32>> {
        let mut prompt = Vec::new();
        if !vocabulary.trim().is_empty() {
            let text = format!(" {}", vocabulary.lines().collect::<Vec<_>>().join(", "));
            let encoding = self
                .tokenizer
                .encode(text, false)
                .map_err(|e| anyhow!("{e}"))?;
            let tokens = encoding.get_ids();
            prompt.push(self.id("<|startofprev|>")?);
            let budget = self.model.config.max_target_positions / 2 - 1;
            prompt.extend_from_slice(&tokens[tokens.len().saturating_sub(budget)..]);
        }
        Ok(prompt)
    }

    fn decode_segment(
        &mut self,
        segment: &[f32],
        prompt: &[u32],
        requested: Option<u32>,
        task: &str,
    ) -> Result<String> {
        let mel = self.frontend.log_mel(segment)?;
        let features = self.model.encoder.forward(&mel, true)?;
        let first = self.logits(&[self.sot], &features, true)?;
        let no_speech_prob = self
            .no_speech
            .map(|id| log_probability(&first, id as usize).exp())
            .unwrap_or(0.);
        let mut tokens = prompt.to_vec();
        tokens.push(self.sot);
        // English-only checkpoints are trained without language and task prefix tokens.
        let mut chinese = false;
        if self.languages.len() > 1 {
            let language = requested.unwrap_or_else(|| {
                self.languages
                    .iter()
                    .map(|(_, id)| *id)
                    .max_by(|a, b| first[*a as usize].total_cmp(&first[*b as usize]))
                    .expect("at least two language tokens")
            });
            chinese = self.tokenizer.token_to_id("<|zh|>") == Some(language);
            tokens.extend([language, self.id(task)?]);
        }
        tokens.push(self.id(whisper::NO_TIMESTAMPS_TOKEN)?);
        let start = tokens.len();
        let mut confidence = 0.;
        while tokens.len() < self.model.config.max_target_positions - 1 {
            let mut logits = self.logits(&tokens, &features, false)?;
            for &id in &self.model.config.suppress_tokens {
                if let Some(x) = logits.get_mut(id as usize) {
                    *x = f32::NEG_INFINITY;
                }
            }
            if tokens.len() == start {
                for &id in &self.begin_suppress {
                    if let Some(x) = logits.get_mut(id as usize) {
                        *x = f32::NEG_INFINITY;
                    }
                }
            }
            // Text tokens precede EOT. Higher IDs encode tasks, languages and timestamps.
            let text_logits = &logits[..=self.eot as usize];
            let (next, _) = text_logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.total_cmp(b))
                .context("empty Whisper vocabulary")?;
            if next == self.eot as usize {
                break;
            }
            confidence += log_probability(text_logits, next);
            tokens.push(next as u32);
        }
        let generated = &tokens[start..];
        if generated.is_empty()
            || (no_speech_prob > 0.6 && confidence / (generated.len() as f32) < -1.)
        {
            return Ok(String::new());
        }
        let text = self.tokenizer
            .decode(generated, false)
            .map_err(|e| anyhow!("{e}"))?;
        // Whisper's Chinese token covers both scripts, including when detected automatically.
        Ok(if chinese && task == whisper::TRANSCRIBE_TOKEN {
            fast2s::convert(&text)
        } else {
            text
        })
    }

    fn id(&self, s: &str) -> Result<u32> {
        self.tokenizer
            .token_to_id(s)
            .with_context(|| format!("missing Whisper token {s}"))
    }

    fn logits(&mut self, tokens: &[u32], features: &Tensor, reset: bool) -> Result<Vec<f32>> {
        let input = Tensor::new(tokens, &Device::Cpu)?.unsqueeze(0)?;
        let hidden = self.model.decoder.forward(&input, features, reset)?;
        let last = hidden.narrow(1, tokens.len() - 1, 1)?;
        Ok(self
            .model
            .decoder
            .final_linear(&last)?
            .i((0, 0))?
            .to_vec1()?)
    }
}

fn log_probability(logits: &[f32], index: usize) -> f32 {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    logits[index] - max - logits.iter().map(|x| (x - max).exp()).sum::<f32>().ln()
}

fn segment_end(samples: &[f32]) -> usize {
    if samples.len() <= WINDOW {
        return samples.len();
    }
    // Prefer a quiet 100 ms boundary near the end of each 30 s acoustic window.
    let search = WINDOW - 3 * SECOND;
    let span = SECOND / 10;
    let mut best = WINDOW;
    let mut minimum = f32::INFINITY;
    for (i, region) in samples[search..WINDOW].chunks_exact(span).enumerate() {
        let energy = region.iter().map(|s| s * s).sum::<f32>();
        if energy <= minimum {
            minimum = energy;
            best = search + i * span + span / 2;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_recordings_split_inside_a_pause_without_skipping_the_tail() {
        let mut samples = vec![0.25; WINDOW + SECOND];
        let pause = (WINDOW - SECOND)..(WINDOW - SECOND + SECOND / 10);
        samples[pause.clone()].fill(0.);
        let cut = segment_end(&samples);
        assert!(pause.contains(&cut));
        let tail = &samples[cut..];
        assert_eq!(segment_end(tail), tail.len());
        assert_eq!(samples[cut..].last(), Some(&0.25));
    }
}
