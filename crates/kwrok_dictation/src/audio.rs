use anyhow::{Result, ensure};
use candle_core::{Device, Tensor};
use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use std::sync::Arc;

pub(crate) struct Frontend {
    window: Vec<f32>,
    filters: Vec<Vec<f32>>,
    fft: Arc<dyn Fft<f32>>,
    frames: usize,
    hop: usize,
}

impl Frontend {
    pub fn new(
        filters: Tensor,
        window: Vec<f32>,
        bands: usize,
        frames: usize,
        rate: usize,
        hop: usize,
    ) -> Result<Self> {
        ensure!(
            rate == 16000 && hop == 160 && frames == 3000 && window.len() == 400,
            "unsupported Whisper audio configuration"
        );
        ensure!(
            filters.dims() == [bands, window.len() / 2 + 1],
            "invalid mel filter dimensions"
        );
        let fft = FftPlanner::new().plan_fft_forward(window.len());
        Ok(Self {
            window,
            filters: filters.to_vec2()?,
            fft,
            frames,
            hop,
        })
    }

    pub fn log_mel(&self, pcm: &[f32]) -> Result<Tensor> {
        let width = self.window.len();
        let mut frame = vec![Complex32::default(); width];
        let mut scratch = vec![Complex32::default(); self.fft.get_inplace_scratch_len()];
        let mut spectrum = vec![0.; width / 2 + 1];
        // Whisper pads the waveform with digital silence before computing the STFT.
        let mut mel = vec![-10f32; self.frames * self.filters.len()];
        let active = self.frames.min((pcm.len() + width / 2).div_ceil(self.hop));
        for i in 0..active {
            let center = i * self.hop;
            for (j, dst) in frame.iter_mut().enumerate() {
                let offset = (center + j) as isize - (width / 2) as isize;
                let idx = offset.unsigned_abs();
                let sample = pcm.get(idx).copied().unwrap_or(0.);
                *dst = Complex32::new(sample * self.window[j], 0.);
            }
            self.fft.process_with_scratch(&mut frame, &mut scratch);
            for (amplitude, frequency) in spectrum.iter_mut().zip(&frame) {
                *amplitude = frequency.norm_sqr();
            }
            for (band, filter) in self.filters.iter().enumerate() {
                let power: f32 = filter.iter().zip(&spectrum).map(|(a, b)| a * b).sum();
                mel[band * self.frames + i] = power.max(1e-10).log10();
            }
        }
        let floor = mel.iter().copied().fold(f32::NEG_INFINITY, f32::max) - 8.;
        for x in &mut mel {
            *x = (x.max(floor) + 4.) / 4.;
        }
        Ok(Tensor::from_vec(
            mel,
            (1, self.filters.len(), self.frames),
            &Device::Cpu,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_silence_has_whisper_floor_across_the_entire_window() -> Result<()> {
        let filters = Tensor::ones((1, 201), candle_core::DType::F32, &Device::Cpu)?;
        let frontend = Frontend::new(filters, vec![1.; 400], 1, 3000, 16_000, 160)?;
        let mel = frontend.log_mel(&[0.; 320])?;
        assert_eq!(mel.dims(), &[1, 1, 3000]);
        assert!(
            mel.flatten_all()?
                .to_vec1::<f32>()?
                .iter()
                .all(|&v| v == -1.5)
        );
        Ok(())
    }
}
