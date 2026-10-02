//! What the listener hears, frame by frame: how much each band of the spectrum
//! jumps (onset strength) and how much energy it holds, from a short-time
//! Fourier transform.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Samples in each analysis window, and between windows.
pub const WINDOW: usize = 2048;
pub const HOP: usize = 256;

/// Edges of the fine bands the spectrum is summed into, in Hz: about a third
/// of an octave each, from the sub to the air.
pub const EDGES: [f32; 25] = [
    30.0, 45.0, 60.0, 80.0, 100.0, 130.0, 170.0, 220.0, 280.0, 360.0, 460.0, 600.0, 780.0, 1_000.0, 1_300.0, 1_700.0,
    2_200.0, 2_800.0, 3_600.0, 4_600.0, 6_000.0, 7_800.0, 10_000.0, 13_000.0, 16_000.0,
];
/// How many fine bands there are.
pub const FINE: usize = EDGES.len() - 1;

/// The ranges the listener names, low to high.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// Kick drums and the sub.
    Low,
    /// A snare's body.
    Body,
    /// A snare's wires and claps.
    Crack,
    /// Hats and cymbals.
    Air,
}

impl Band {
    pub const ALL: [Band; 4] = [Band::Low, Band::Body, Band::Crack, Band::Air];

    /// Lowest and highest frequency, in Hz.
    pub const fn range(self) -> (f32, f32) {
        match self {
            Band::Low => (30.0, 130.0),
            Band::Body => (170.0, 600.0),
            Band::Crack => (1_300.0, 6_000.0),
            Band::Air => (6_000.0, 16_000.0),
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Per fine band, per frame: onset strength and energy. Frame `i` is centred
/// on sample `i * HOP`.
#[derive(Clone, Debug)]
pub struct Spectrogram {
    pub sample_rate: u32,
    pub frames: usize,
    /// Half-wave rectified rise of the log-magnitude spectrum, averaged over each fine band.
    pub fine_flux: Vec<Vec<f32>>,
    /// Sum of squared magnitudes over each fine band.
    pub fine_energy: Vec<Vec<f32>>,
    /// The same, summed over the named bands.
    pub flux: [Vec<f32>; 4],
    pub energy: [Vec<f32>; 4],
}

impl Spectrogram {
    /// Frames per second.
    pub fn rate(&self) -> f64 {
        f64::from(self.sample_rate) / HOP as f64
    }

    /// The time at the centre of frame `i`, in seconds.
    pub fn time(&self, frame: f64) -> f64 {
        frame / self.rate()
    }

    pub fn flux(&self, band: Band) -> &[f32] {
        &self.flux[band.index()]
    }

    pub fn energy(&self, band: Band) -> &[f32] {
        &self.energy[band.index()]
    }

    /// The fine bands lying between two frequencies.
    pub fn fine_between(&self, low: f32, high: f32) -> std::ops::Range<usize> {
        let first = EDGES.iter().position(|&e| e >= low).unwrap_or(FINE).min(FINE);
        let last = EDGES.iter().rposition(|&e| e <= high).unwrap_or(0);
        first..last.max(first)
    }

    /// Energy summed over the fine bands between two frequencies, frame by frame.
    pub fn energy_between(&self, low: f32, high: f32) -> Vec<f32> {
        let bands = self.fine_between(low, high);
        (0..self.frames)
            .map(|i| bands.clone().map(|b| self.fine_energy[b][i]).sum())
            .collect()
    }

    /// Onset strength averaged over the fine bands between two frequencies.
    pub fn flux_between(&self, low: f32, high: f32) -> Vec<f32> {
        let bands = self.fine_between(low, high);
        let count = bands.len().max(1) as f32;
        (0..self.frames)
            .map(|i| bands.clone().map(|b| self.fine_flux[b][i]).sum::<f32>() / count)
            .collect()
    }
}

/// Listens to `mono` at `sample_rate`.
pub fn analyse(mono: &[f32], sample_rate: u32) -> Spectrogram {
    let fft: Arc<dyn Fft<f32>> = FftPlanner::new().plan_fft_forward(WINDOW);
    let window: Vec<f32> = (0..WINDOW)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos())
        .collect();
    let bin_hz = sample_rate as f32 / WINDOW as f32;
    // Each fine band's bins: from its lower edge up to (not including) its upper one.
    let bins: Vec<(usize, usize)> = EDGES
        .windows(2)
        .map(|edge| {
            let first = ((edge[0] / bin_hz).ceil() as usize).max(1);
            let last = ((edge[1] / bin_hz).ceil() as usize).min(WINDOW / 2).max(first + 1);
            (first, last)
        })
        .collect();
    let frames = mono.len() / HOP + 1;
    let mut fine_flux: Vec<Vec<f32>> = (0..FINE).map(|_| Vec::with_capacity(frames)).collect();
    let mut fine_energy: Vec<Vec<f32>> = (0..FINE).map(|_| Vec::with_capacity(frames)).collect();
    let mut buffer = vec![Complex::new(0.0f32, 0.0); WINDOW];
    let mut scratch = vec![Complex::new(0.0f32, 0.0); fft.get_inplace_scratch_len()];
    let mut previous = vec![0.0f32; WINDOW / 2 + 1];
    let mut current = vec![0.0f32; WINDOW / 2 + 1];
    for frame in 0..frames {
        let centre = frame * HOP;
        for (i, slot) in buffer.iter_mut().enumerate() {
            let at = (centre + i).checked_sub(WINDOW / 2);
            let x = at.and_then(|at| mono.get(at)).copied().unwrap_or(0.0);
            *slot = Complex::new(x * window[i], 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        for (k, slot) in current.iter_mut().enumerate() {
            *slot = buffer[k].norm();
        }
        for (b, &(first, last)) in bins.iter().enumerate() {
            let mut rise = 0.0f32;
            let mut power = 0.0f32;
            for k in first..last {
                // Log compression: quiet details count, loud ones don't swamp them.
                let now = (1.0 + 100.0 * current[k]).ln();
                let before = (1.0 + 100.0 * previous[k]).ln();
                rise += (now - before).max(0.0);
                power += current[k] * current[k];
            }
            fine_flux[b].push(rise / (last - first) as f32);
            fine_energy[b].push(power);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let mut spec = Spectrogram {
        sample_rate,
        frames,
        fine_flux,
        fine_energy,
        flux: Default::default(),
        energy: Default::default(),
    };
    for band in Band::ALL {
        let (low, high) = band.range();
        spec.flux[band.index()] = spec.flux_between(low, high);
        spec.energy[band.index()] = spec.energy_between(low, high);
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_low_thump_moves_the_low_band_and_a_tick_the_air() {
        let sr = 48_000;
        let mut audio = vec![0.0f32; sr as usize];
        // A 60 Hz thump at 0.25 s, a burst of 10 kHz at 0.75 s.
        for i in 0..4_800 {
            let t = i as f32 / sr as f32;
            audio[12_000 + i] += (std::f32::consts::TAU * 60.0 * t).sin() * (-t / 0.03).exp();
            audio[36_000 + i] += (std::f32::consts::TAU * 10_000.0 * t).sin() * (-t / 0.005).exp();
        }
        let spec = analyse(&audio, sr);
        let peak = |band: Band| {
            let flux = spec.flux(band);
            let (frame, _) = flux
                .iter()
                .enumerate()
                .fold((0, 0.0f32), |best, (i, &x)| if x > best.1 { (i, x) } else { best });
            spec.time(frame as f64)
        };
        assert!((peak(Band::Low) - 0.25).abs() < 0.03, "thump at {}", peak(Band::Low));
        assert!((peak(Band::Air) - 0.75).abs() < 0.03, "tick at {}", peak(Band::Air));
    }
}
