//! Offline soundtrack for the first-run film. Opening or releasing an output device must never
//! block the window, including when the user skips before Core Audio has finished starting.

use anyhow::{Context, Result, bail};
use cpal::{
    Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

// Canonical 16-bit stereo PCM WAV, with a 44-byte RIFF header.
const SOUNDTRACK: &[u8] = include_bytes!("../../../assets/audio/link-start.wav");
const HEADER: usize = 44;
const RATE: usize = 24_000;
const FRAME_BYTES: usize = 4;
const FRAMES: usize = (SOUNDTRACK.len() - HEADER) / FRAME_BYTES;

pub(crate) struct IntroAudio {
    cancelled: Arc<AtomicBool>,
    // Dropping the only sender wakes the audio thread even on hosts with no audio callbacks.
    _lifetime: mpsc::Sender<()>,
}

impl IntroAudio {
    pub(crate) fn start() -> Self {
        let started = Instant::now();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&cancelled);
        let (lifetime, exits) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("kwrok-intro-audio".into())
            .spawn(move || {
                if let Err(error) = play(started, stopped, exits) {
                    log::debug!("Intro audio unavailable: {error}");
                }
            });
        if let Err(error) = worker {
            log::debug!("Could not start intro audio thread: {error}");
        }
        Self {
            cancelled,
            _lifetime: lifetime,
        }
    }
}

impl Drop for IntroAudio {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

fn play(started: Instant, stopped: Arc<AtomicBool>, exits: mpsc::Receiver<()>) -> Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no audio output")?;
    let format = device.default_output_config()?;
    let config = format.config();
    if stopped.load(Ordering::Acquire) || config.channels == 0 || config.sample_rate.0 == 0 {
        return Ok(());
    }
    let stream = match format.sample_format() {
        SampleFormat::F32 => output::<f32>(&device, &config, started, &stopped),
        SampleFormat::F64 => output::<f64>(&device, &config, started, &stopped),
        SampleFormat::I8 => output::<i8>(&device, &config, started, &stopped),
        SampleFormat::I16 => output::<i16>(&device, &config, started, &stopped),
        SampleFormat::I24 => output::<cpal::I24>(&device, &config, started, &stopped),
        SampleFormat::I32 => output::<i32>(&device, &config, started, &stopped),
        SampleFormat::I64 => output::<i64>(&device, &config, started, &stopped),
        SampleFormat::U8 => output::<u8>(&device, &config, started, &stopped),
        SampleFormat::U16 => output::<u16>(&device, &config, started, &stopped),
        SampleFormat::U32 => output::<u32>(&device, &config, started, &stopped),
        SampleFormat::U64 => output::<u64>(&device, &config, started, &stopped),
        other => bail!("unsupported device sample format {other}"),
    }?;
    if !stopped.load(Ordering::Acquire) {
        stream.play()?;
        let duration = Duration::from_secs_f64(FRAMES as f64 / RATE as f64);
        let _ = exits.recv_timeout(duration.saturating_sub(started.elapsed()));
    }
    Ok(())
}

fn output<T: SizedSample + FromSample<f32>>(
    device: &Device,
    config: &StreamConfig,
    started: Instant,
    stopped: &Arc<AtomicBool>,
) -> Result<Stream> {
    let channels = usize::from(config.channels);
    let step = RATE as f64 / f64::from(config.sample_rate.0);
    let stopped = Arc::clone(stopped);
    let mut cursor = None;
    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], info| {
            data.fill(T::EQUILIBRIUM);
            if stopped.load(Ordering::Acquire) {
                return;
            }
            let clock = cursor.get_or_insert_with(|| {
                let times = info.timestamp();
                let latency = times
                    .playback
                    .duration_since(&times.callback)
                    .unwrap_or_default();
                MixClock::new(started.elapsed() + latency)
            });
            for frame in data.chunks_exact_mut(channels) {
                let position = clock.source_frame();
                let i = position as usize;
                if i + 1 >= FRAMES {
                    break;
                }
                let a = stereo(i);
                let b = stereo(i + 1);
                let t = (position - i as f64) as f32;
                let left = a[0] + t * (b[0] - a[0]);
                let right = a[1] + t * (b[1] - a[1]);
                if channels == 1 {
                    frame[0] = T::from_sample((left + right) * 0.5);
                } else {
                    frame[0] = T::from_sample(left);
                    frame[1] = T::from_sample(right);
                }
                clock.device_frame += step;
            }
        },
        |error| log::debug!("Intro audio output: {error}"),
        None,
    )?)
}

// A cold output route can take hundreds of milliseconds to open. Preserve the syllables
// of the launch cue and ease back onto the film clock during the unvoiced tunnel.
struct MixClock {
    device_frame: f64,
    launch_delay: f64,
}

impl MixClock {
    fn new(audible_at: Duration) -> Self {
        let elapsed = audible_at.as_secs_f64();
        let delay = if elapsed < 1.0 {
            (elapsed - 0.04).max(0.)
        } else {
            0.
        };
        Self {
            // If a route appears after the welcome has already begun, keep the film silent.
            device_frame: if elapsed > 3.52 {
                FRAMES as f64
            } else {
                elapsed * RATE as f64
            },
            launch_delay: delay * RATE as f64,
        }
    }

    fn source_frame(&self) -> f64 {
        let start = 0.95 * RATE as f64 + self.launch_delay;
        let end = 3.30 * RATE as f64;
        let t = ((self.device_frame - start) / (end - start)).clamp(0., 1.);
        let ease = t * t * (3. - 2. * t);
        self.device_frame - self.launch_delay * (1. - ease)
    }
}

fn stereo(frame: usize) -> [f32; 2] {
    let i = HEADER + frame * FRAME_BYTES;
    let sample = |n| f32::from(i16::from_le_bytes([SOUNDTRACK[n], SOUNDTRACK[n + 1]])) / 32768.;
    [sample(i), sample(i + 2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_device_keeps_the_launch_phrase_and_lands_on_the_title() {
        let mut clock = MixClock::new(Duration::from_millis(280));
        assert!(clock.source_frame() < 0.055 * RATE as f64);
        clock.device_frame += 0.8 * RATE as f64;
        assert!((clock.source_frame() / RATE as f64 - 0.84).abs() < 0.001);
        clock.device_frame = 3.32 * RATE as f64;
        assert_eq!(clock.source_frame(), clock.device_frame);
        assert!(MixClock::new(Duration::from_secs(4)).source_frame() >= FRAMES as f64);
    }

    #[test]
    fn offline_mix_matches_film_clock_and_sample_layout() {
        assert_eq!(&SOUNDTRACK[..4], b"RIFF");
        assert_eq!(&SOUNDTRACK[8..16], b"WAVEfmt ");
        assert_eq!(&SOUNDTRACK[20..24], &[1, 0, 2, 0]); // PCM, stereo
        assert_eq!(&SOUNDTRACK[24..28], &(RATE as u32).to_le_bytes());
        assert_eq!(&SOUNDTRACK[34..40], b"\x10\0data");
        assert_eq!(FRAMES, RATE * 11 / 2);
        assert_eq!(stereo(0), [0.; 2]);
        assert_eq!(stereo(FRAMES - 1), [0.; 2]);
        // The channel motion is audible in the tunnel; both spoken cues have a center image.
        for (at, end) in [(0.05, 0.85), (3.52, 5.45)] {
            assert!(
                ((RATE as f32 * at) as usize..(RATE as f32 * end) as usize)
                    .any(|i| stereo(i).iter().all(|v| v.abs() > 0.2))
            );
        }
        assert!((RATE..RATE * 3).any(|i| (stereo(i)[0] - stereo(i)[1]).abs() > 0.12));
    }
}
