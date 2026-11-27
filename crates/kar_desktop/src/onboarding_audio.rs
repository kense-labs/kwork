//! Non-blocking audio engine for the first-run Link Start film and tactile SAO-inspired UI chimes.
//! Opening or releasing an output device never blocks the GPUI main thread, including when the
//! user skips before Core Audio has finished starting or when no output route exists.

use anyhow::{Context, Result, bail};
use cpal::{
    Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    f32::consts::TAU,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

// Canonical 16-bit stereo PCM WAV (32 kHz, 6.20s), with a 44-byte RIFF header.
const SOUNDTRACK: &[u8] = include_bytes!("../../../assets/audio/link-start.wav");
const HEADER: usize = 44;
pub(crate) const RATE: usize = 32_000;
const FRAME_BYTES: usize = 4;
const FRAMES: usize = (SOUNDTRACK.len() - HEADER) / FRAME_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum UiSfx {
    /// Crisp two-note ascending SAO crystal bell (agent select / primary action).
    Confirm = 1,
    /// Delicate single glass tap (back / rescan).
    Navigate = 2,
    /// Soft descending glass chime (skip / dismiss).
    Dismiss = 3,
    /// Warm three-note Cardinal harmonic lock when entering the first conversation.
    Connected = 4,
}

impl UiSfx {
    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Confirm),
            2 => Some(Self::Navigate),
            3 => Some(Self::Dismiss),
            4 => Some(Self::Connected),
            _ => None,
        }
    }

    fn tail_duration(self) -> Duration {
        match self {
            Self::Connected => Duration::from_millis(420),
            Self::Confirm | Self::Dismiss => Duration::from_millis(280),
            Self::Navigate => Duration::from_millis(190),
        }
    }
}

struct SharedState {
    film_run: Mutex<Option<(u64, Instant)>>,
    film_stopped: AtomicBool,
    closing: AtomicBool,
    sfx_cmd: AtomicU64,
    next_seq: AtomicU64,
}

/// Kept alive only while the onboarding surface is active. Pre-warming on the Welcome screen
/// eliminates Core Audio's 150-300 ms route-open latency when the user clicks "Sign in".
pub(crate) struct IntroAudio {
    state: Arc<SharedState>,
    wake: mpsc::Sender<()>,
}

impl IntroAudio {
    pub(crate) fn prewarm() -> Self {
        Self::new_inner(false)
    }

    pub(crate) fn start_film() -> Self {
        Self::new_inner(true)
    }

    fn new_inner(start_immediately: bool) -> Self {
        let now = Instant::now();
        let state = Arc::new(SharedState {
            film_run: Mutex::new(start_immediately.then_some((1, now))),
            film_stopped: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            sfx_cmd: AtomicU64::new(0),
            next_seq: AtomicU64::new(2),
        });
        let worker_state = Arc::clone(&state);
        let (wake, rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("kwrok-intro-audio".into())
            .spawn(move || {
                if let Err(error) = run_audio_worker(worker_state, rx) {
                    log::debug!("Intro audio unavailable: {error}");
                }
            });
        if let Err(error) = worker {
            log::debug!("Could not start intro audio thread: {error}");
        }
        Self { state, wake }
    }

    pub(crate) fn play_film(&self) {
        let seq = self.state.next_seq.fetch_add(1, Ordering::Relaxed);
        self.state.film_stopped.store(false, Ordering::Release);
        if let Ok(mut slot) = self.state.film_run.lock() {
            *slot = Some((seq, Instant::now()));
        }
        let _ = self.wake.send(());
    }

    pub(crate) fn stop_film(&self) {
        self.state.film_stopped.store(true, Ordering::Release);
        if let Ok(mut slot) = self.state.film_run.lock() {
            *slot = None;
        }
    }

    pub(crate) fn play_sfx(&self, sfx: UiSfx) {
        let seq = self.state.next_seq.fetch_add(1, Ordering::Relaxed) & 0x00ff_ffff_ffff_ffff;
        let packed = (seq << 8) | u64::from(sfx as u8);
        self.state.sfx_cmd.store(packed, Ordering::Release);
        let _ = self.wake.send(());
    }
}

impl Drop for IntroAudio {
    fn drop(&mut self) {
        self.state.film_stopped.store(true, Ordering::Release);
        self.state.closing.store(true, Ordering::Release);
        let _ = self.wake.send(());
    }
}

fn run_audio_worker(state: Arc<SharedState>, rx: mpsc::Receiver<()>) -> Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no audio output")?;
    let format = device.default_output_config()?;
    let config = format.config();
    if state.closing.load(Ordering::Acquire) && state.sfx_cmd.load(Ordering::Acquire) == 0
        || config.channels == 0
        || config.sample_rate.0 == 0
    {
        return Ok(());
    }
    let stream = match format.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, &state),
        SampleFormat::F64 => build_stream::<f64>(&device, &config, &state),
        SampleFormat::I8 => build_stream::<i8>(&device, &config, &state),
        SampleFormat::I16 => build_stream::<i16>(&device, &config, &state),
        SampleFormat::I24 => build_stream::<cpal::I24>(&device, &config, &state),
        SampleFormat::I32 => build_stream::<i32>(&device, &config, &state),
        SampleFormat::I64 => build_stream::<i64>(&device, &config, &state),
        SampleFormat::U8 => build_stream::<u8>(&device, &config, &state),
        SampleFormat::U16 => build_stream::<u16>(&device, &config, &state),
        SampleFormat::U32 => build_stream::<u32>(&device, &config, &state),
        SampleFormat::U64 => build_stream::<u64>(&device, &config, &state),
        other => bail!("unsupported device sample format {other}"),
    }?;
    stream.play()?;

    let mut last_seen_sfx = 0u64;
    let mut sfx_deadline: Option<Instant> = None;
    loop {
        let cmd = state.sfx_cmd.load(Ordering::Acquire);
        if cmd != 0 && cmd != last_seen_sfx {
            last_seen_sfx = cmd;
            if let Some(kind) = UiSfx::from_code((cmd & 0xff) as u8) {
                let until = Instant::now() + kind.tail_duration();
                sfx_deadline = Some(sfx_deadline.map_or(until, |prev| prev.max(until)));
            }
        }
        if state.closing.load(Ordering::Acquire) {
            if let Some(until) = sfx_deadline
                && let Some(rem) = until.checked_duration_since(Instant::now())
            {
                thread::sleep(rem.min(Duration::from_millis(450)));
            }
            break;
        }
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let cmd = state.sfx_cmd.load(Ordering::Acquire);
                if cmd != 0
                    && cmd != last_seen_sfx
                    && let Some(kind) = UiSfx::from_code((cmd & 0xff) as u8)
                {
                    thread::sleep(kind.tail_duration());
                } else if let Some(until) = sfx_deadline
                    && let Some(rem) = until.checked_duration_since(Instant::now())
                {
                    thread::sleep(rem.min(Duration::from_millis(450)));
                }
                break;
            }
        }
    }
    Ok(())
}

struct ActiveSfx {
    kind: UiSfx,
    sample_pos: usize,
}

fn build_stream<T: SizedSample + FromSample<f32>>(
    device: &Device,
    config: &StreamConfig,
    state: &Arc<SharedState>,
) -> Result<Stream> {
    let channels = usize::from(config.channels);
    let dev_sr = f64::from(config.sample_rate.0);
    let step = RATE as f64 / dev_sr;
    let inv_dev_sr = 1.0 / dev_sr as f32;
    let state = Arc::clone(state);

    let mut active_film_seq = 0u64;
    let mut clock: Option<MixClock> = None;
    let mut active_sfx_seq = 0u64;
    let mut sfx_voice: Option<ActiveSfx> = None;

    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], info| {
            data.fill(T::EQUILIBRIUM);

            // Check for newly triggered UI chime
            let sfx_packed = state.sfx_cmd.load(Ordering::Acquire);
            if sfx_packed != 0 && sfx_packed != active_sfx_seq {
                active_sfx_seq = sfx_packed;
                sfx_voice = UiSfx::from_code((sfx_packed & 0xff) as u8).map(|kind| ActiveSfx {
                    kind,
                    sample_pos: 0,
                });
            }

            // Check film playback state
            if state.film_stopped.load(Ordering::Acquire) {
                clock = None;
            } else if let Ok(guard) = state.film_run.try_lock() {
                match *guard {
                    Some((seq, started)) if seq != active_film_seq => {
                        active_film_seq = seq;
                        let times = info.timestamp();
                        let latency = times
                            .playback
                            .duration_since(&times.callback)
                            .unwrap_or_default();
                        clock = Some(MixClock::new(started.elapsed() + latency));
                    }
                    None => {
                        clock = None;
                    }
                    _ => {}
                }
            }

            if clock.is_none() && sfx_voice.is_none() {
                return;
            }

            for frame in data.chunks_exact_mut(channels) {
                let mut left = 0.0f32;
                let mut right = 0.0f32;

                if let Some(clk) = &mut clock {
                    let position = clk.source_frame();
                    let i = position as usize;
                    if i + 1 < FRAMES {
                        let a = stereo(i);
                        let b = stereo(i + 1);
                        let frac = (position - i as f64) as f32;
                        left += a[0] + frac * (b[0] - a[0]);
                        right += a[1] + frac * (b[1] - a[1]);
                        clk.device_frame += step;
                    } else {
                        clock = None;
                    }
                }

                if let Some(sfx) = &mut sfx_voice {
                    let t = sfx.sample_pos as f32 * inv_dev_sr;
                    sfx.sample_pos += 1;
                    if let Some([sl, sr]) = sample_ui_sfx(sfx.kind, t) {
                        left += sl;
                        right += sr;
                    } else {
                        sfx_voice = None;
                    }
                }

                let left = left.clamp(-0.98, 0.98);
                let right = right.clamp(-0.98, 0.98);
                if channels == 1 {
                    frame[0] = T::from_sample((left + right) * 0.5);
                } else {
                    frame[0] = T::from_sample(left);
                    frame[1] = T::from_sample(right);
                }
            }
        },
        |error| log::debug!("Intro audio output: {error}"),
        None,
    )?)
}

/// Real-time pure-Rust SAO crystalline glass UI chime synthesizer.
pub(crate) fn sample_ui_sfx(kind: UiSfx, t: f32) -> Option<[f32; 2]> {
    let crystal_note = |dt: f32, freq: f32, dur: f32, amp: f32| -> f32 {
        if dt <= 0.0 || dt >= dur {
            return 0.0;
        }
        let attack = 1.0 - (-dt / 0.0016).exp();
        let env = attack * (-dt / (dur * 0.34)).exp();
        let mod_idx = (-dt / 0.032).exp() * 0.72;
        let fm = (TAU * freq * 2.0 * dt).sin() * mod_idx;
        let body = (TAU * freq * dt + fm).sin() * env;
        let glass = (TAU * freq * 2.756 * dt).sin() * attack * (-dt / 0.042).exp() * 0.14;
        let tail_fade = 1.0 - ((dt - dur * 0.78) / (dur * 0.22)).clamp(0.0, 1.0);
        (body + glass) * amp * tail_fade
    };

    match kind {
        UiSfx::Confirm => {
            if t >= 0.26 {
                return None;
            }
            let n1 = crystal_note(t, 1479.98, 0.20, 0.12); // F#6
            let n2 = crystal_note(t - 0.042, 1975.53, 0.21, 0.14); // B6
            Some([n1 * 1.08 + n2 * 0.88, n1 * 0.88 + n2 * 1.08])
        }
        UiSfx::Navigate => {
            if t >= 0.17 {
                return None;
            }
            let n = crystal_note(t, 1318.51, 0.16, 0.095); // E6
            Some([n, n * 0.96])
        }
        UiSfx::Dismiss => {
            if t >= 0.26 {
                return None;
            }
            let n1 = crystal_note(t, 1479.98, 0.18, 0.10); // F#6
            let n2 = crystal_note(t - 0.046, 1108.73, 0.20, 0.10); // C#6
            Some([n1 * 0.92 + n2 * 1.06, n1 * 1.06 + n2 * 0.92])
        }
        UiSfx::Connected => {
            if t >= 0.40 {
                return None;
            }
            let n1 = crystal_note(t, 987.77, 0.30, 0.095); // B5
            let n2 = crystal_note(t - 0.045, 1479.98, 0.32, 0.105); // F#6
            let n3 = crystal_note(t - 0.092, 1975.53, 0.30, 0.12); // B6
            Some([n1 * 1.08 + n2 * 0.90 + n3, n1 * 0.88 + n2 * 1.10 + n3])
        }
    }
}

// A cold output route (for example, replaying from Settings before pre-warming) can take
// hundreds of milliseconds to open. Preserve the syllables of "Link Start!" and ease back
// onto the film clock during the unvoiced hyperspace wormhole before the Welcome reveal.
struct MixClock {
    device_frame: f64,
    launch_delay: f64,
}

impl MixClock {
    fn new(audible_at: Duration) -> Self {
        let elapsed = audible_at.as_secs_f64();
        let delay = if elapsed < 1.0 {
            (elapsed - 0.03).max(0.)
        } else {
            0.
        };
        Self {
            // If a route appears after the Welcome greeting has already begun, keep silent.
            device_frame: if elapsed > 3.80 {
                FRAMES as f64
            } else {
                elapsed * RATE as f64
            },
            launch_delay: delay * RATE as f64,
        }
    }

    fn source_frame(&self) -> f64 {
        let start = 0.96 * RATE as f64 + self.launch_delay;
        let end = 3.50 * RATE as f64;
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
        assert!((clock.source_frame() / RATE as f64 - 0.83).abs() < 0.001);
        clock.device_frame = 3.55 * RATE as f64;
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
        assert_eq!(FRAMES, RATE * 31 / 5); // 6.20 seconds @ 32 kHz
        assert_eq!(stereo(0), [0.; 2]);
        assert_eq!(stereo(FRAMES - 1), [0.; 2]);
        // Both spoken cues ("Link Start!" and "Welcome to Kense Agent Online.") have strong center energy.
        for (at, end) in [(0.08, 0.82), (3.86, 5.85)] {
            assert!(
                ((RATE as f32 * at) as usize..(RATE as f32 * end) as usize)
                    .any(|i| stereo(i).iter().all(|v| v.abs() > 0.2))
            );
        }
        // Stereo motion is audible throughout the prismatic wormhole.
        assert!((RATE..RATE * 3).any(|i| (stereo(i)[0] - stereo(i)[1]).abs() > 0.08));
    }

    #[test]
    fn realtime_sao_ui_chimes_are_bounded_and_decay_to_none() {
        for kind in [
            UiSfx::Confirm,
            UiSfx::Navigate,
            UiSfx::Dismiss,
            UiSfx::Connected,
        ] {
            let mut peak = 0.0f32;
            for step in 0..400 {
                let t = step as f32 * 0.001;
                if let Some([l, r]) = sample_ui_sfx(kind, t) {
                    peak = peak.max(l.abs()).max(r.abs());
                }
            }
            assert!(peak > 0.05 && peak < 0.5, "{kind:?} peak={peak}");
            assert!(sample_ui_sfx(kind, 0.50).is_none());
        }
    }
}
