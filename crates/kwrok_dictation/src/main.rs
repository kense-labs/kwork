//! Command-line interface for Kar's local speech service.
#![forbid(unsafe_code)]

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use kwrok_dictation::Recognizer;
use std::{
    fs,
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

const SAMPLE_RATE: u32 = 16_000;
const MAX_SAMPLES: u32 = SAMPLE_RATE * 300;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Task {
    Transcribe,
    Translate,
}

#[derive(Parser)]
#[command(
    name = "kwrok-dictation",
    version,
    about = "Offline Whisper dictation on the CPU"
)]
struct Args {
    #[arg(long, exclusive = true)]
    list_devices: bool,
    #[arg(long, required_unless_present = "list_devices")]
    model: Option<PathBuf>,
    /// A 16 kHz mono PCM WAV file.
    #[arg(required_unless_present_any = ["batch", "list_devices", "preview"], conflicts_with_all = ["batch", "preview"])]
    input: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["batch", "preview"])]
    output: Option<PathBuf>,
    /// Recognize successive length-prefixed mono f32 windows from standard input.
    #[arg(long, conflicts_with = "batch")]
    preview: bool,
    #[arg(long, requires = "batch_jsonl")]
    batch: Option<PathBuf>,
    #[arg(long, requires = "batch")]
    batch_jsonl: bool,
    #[arg(long, value_parser = ["cpu", "auto"])]
    backend: Option<String>,
    #[arg(long)]
    language: Option<String>,
    #[arg(long, value_enum, default_value = "transcribe")]
    task: Task,
    #[arg(long)]
    vocabulary_file: Option<PathBuf>,
    #[arg(long)]
    quiet: bool,
}

fn read_audio(path: &Path) -> Result<Vec<f32>> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 1
            && spec.sample_rate == SAMPLE_RATE
            && spec.sample_format == hound::SampleFormat::Int
            && spec.bits_per_sample == 16,
        "expected 16 kHz mono signed 16-bit PCM WAV"
    );
    ensure!(
        (1..=MAX_SAMPLES).contains(&reader.len()),
        "expected up to five minutes of nonempty audio"
    );
    Ok(reader
        .samples::<i16>()
        .map(|sample| sample.map(|s| f32::from(s) / 32768.0))
        .collect::<std::result::Result<_, _>>()?)
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.list_devices {
        println!("[0] CPU\n  type=cpu");
        return Ok(());
    }
    let vocabulary = args
        .vocabulary_file
        .as_deref()
        .map(fs::read_to_string)
        .transpose()?
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let started = Instant::now();
    let model = args.model.as_deref().context("missing model path")?;
    let mut recognizer = Recognizer::load(model)
        .with_context(|| format!("load Whisper model {}", model.display()))?;
    let mut decode = |pcm: &[f32]| -> Result<String> {
        recognizer.transcribe(
            pcm,
            args.language.as_deref().filter(|s| *s != "auto"),
            matches!(args.task, Task::Translate),
            &vocabulary,
        )
    };
    if args.preview {
        // One reply per window, so the recorder never queues stale work. EOF closes the session.
        let mut input = io::stdin().lock();
        let mut out = io::stdout().lock();
        while !input.fill_buf()?.is_empty() {
            let mut header = [0; 4];
            input.read_exact(&mut header)?;
            let frames = u32::from_le_bytes(header);
            ensure!(
                (1..=30 * SAMPLE_RATE).contains(&frames),
                "invalid preview window"
            );
            let mut bytes = vec![0; frames as usize * 4];
            input.read_exact(&mut bytes)?;
            let pcm: Vec<_> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|s| f32::from_le_bytes(*s))
                .collect();
            writeln!(out, "{}", serde_json::to_string(&decode(&pcm)?)?)?;
            out.flush()?;
        }
    } else if let Some(batch) = args.batch {
        let paths = fs::read_to_string(batch)?;
        let mut out = io::stdout().lock();
        // Kar waits for this line to verify the chosen model loads, then closes the process.
        writeln!(
            out,
            "{}",
            serde_json::json!({"type":"batch_header", "load_ms": started.elapsed().as_millis()})
        )?;
        out.flush()?;
        for input in paths.lines().filter(|s| !s.is_empty()) {
            let text = decode(&read_audio(Path::new(input))?)?;
            writeln!(out, "{}", serde_json::json!({"file": input, "text": text}))?;
        }
    } else {
        let text = decode(&read_audio(
            args.input.as_deref().context("missing audio")?,
        )?)?;
        if let Some(output) = args.output {
            fs::write(output, text)?;
        } else {
            println!("{text}");
        }
    }
    Ok(())
}
