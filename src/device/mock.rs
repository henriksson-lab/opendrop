//! Mock spectrometer backend — synthesizes plausible spectra with no hardware.
//!
//! The mock mimics the ND-1000 workflow: a [`blank`](MockSpectrometer::blank)
//! must be recorded before [`measure`](MockSpectrometer::measure) will return a
//! sample. Each `measure` produces a DNA-like absorbance curve (10 mm-normalized)
//! peaking near 260 nm with a 230 nm shoulder and a near-zero 340 nm baseline,
//! plus a little per-call noise. Randomness comes from a small internal LCG (a
//! deterministic counter-based generator), so no `rand`/`Math::random` is needed
//! and results still vary from call to call.

use std::thread::sleep;
use std::time::Duration;

use crate::measure::spectrum::{Spectrum, SPECTRUM_LEN, WL_START_NM, WL_STEP_NM};

use crate::device::{DeviceError, DeviceInfo, Spectrometer};

/// Simulated acquisition time for a blank or a measurement. A real ND-1000
/// takes roughly a second to integrate a spectrum; the mock sleeps for the
/// same span so the GUI's "Measuring…" affordance is meaningful.
pub const DEFAULT_ACQUIRE_DELAY: Duration = Duration::from_secs(1);

/// Largest 10 mm-equivalent absorbance a good blank re-read may show in the
/// 220–350 nm window. A clean reference reads flat and near zero; anything
/// above this signals a bad blank (bubble, dirty pedestal, …).
const BLANK_MAX_ABS: f64 = 0.15;

/// A synthetic spectrometer that fabricates plausible nucleic-acid spectra.
pub struct MockSpectrometer {
    info: DeviceInfo,
    /// Whether a blank reference has been recorded this session.
    has_blank: bool,
    /// State of the internal LCG used for per-call variation.
    rng_state: u64,
    /// Monotonic counter of samples measured (drives amplitude variation).
    sample_counter: u64,
    /// Simulated per-acquisition delay (`blank`/`measure`). Tests set this to
    /// `Duration::ZERO` to keep the suite fast.
    delay: Duration,
    /// Test hook: when set, the next blank re-read comes back out of range so
    /// [`verify_blank`](Spectrometer::verify_blank) reports a bad blank.
    simulate_bad_blank: bool,
}

impl MockSpectrometer {
    /// Create a new mock instrument with a plausible identity.
    pub fn new() -> Self {
        Self {
            info: DeviceInfo {
                model: "ND-1000 (mock)".to_string(),
                serial: "MOCK-08708".to_string(),
                config: "3.8.1 B 08708 -0.79/128/16".to_string(),
            },
            has_blank: false,
            // Arbitrary non-zero seed for the LCG.
            rng_state: 0x2545_F491_4F6C_DD1D,
            sample_counter: 0,
            delay: DEFAULT_ACQUIRE_DELAY,
            simulate_bad_blank: false,
        }
    }

    /// Override the simulated acquisition delay (default
    /// [`DEFAULT_ACQUIRE_DELAY`]). Used by tests to run without the 1 s wait.
    pub fn set_delay(&mut self, delay: Duration) {
        self.delay = delay;
    }

    /// Test hook: force the next [`verify_blank`](Spectrometer::verify_blank)
    /// to fail, as if the recorded reference were bad.
    pub fn set_simulate_bad_blank(&mut self, bad: bool) {
        self.simulate_bad_blank = bad;
    }

    /// Advance the LCG and return a pseudo-random float in `[0.0, 1.0)`.
    ///
    /// Uses the well-known PCG/Numerical-Recipes multiplier + increment. This is
    /// deterministic and self-contained (no external RNG / `Math::random`).
    fn next_unit(&mut self) -> f64 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // Take the top 53 bits for a uniform double in [0, 1).
        ((self.rng_state >> 11) as f64) / ((1u64 << 53) as f64)
    }

    /// Symmetric noise in `[-mag, mag)`.
    fn next_noise(&mut self, mag: f64) -> f64 {
        (self.next_unit() * 2.0 - 1.0) * mag
    }

    /// Synthesize a DNA-like 10 mm-normalized absorbance spectrum.
    ///
    /// Shape: a Gaussian at 260 nm (σ ≈ 18 nm → A260/A280 ≈ 1.85), a smaller
    /// Gaussian "shoulder" at 230 nm (→ A260/A230 ≈ 2.1), and a flat ~0 baseline
    /// above ~320 nm. Peak height varies per call so the reported ng/µL moves.
    fn synthesize(&mut self) -> Spectrum {
        // A260 peak height (10 mm-equiv AU): base plus counter-driven drift plus
        // a little jitter, so concentration reads ~900–1800 ng/µL for dsDNA.
        let drift = (self.sample_counter % 9) as f64 * 1.8;
        let a260_peak = 18.0 + drift + self.next_noise(2.0);

        // Gaussian parameters.
        let main_sigma = 18.0_f64; // controls 260/280
        let shoulder_amp = 0.22 * a260_peak; // controls 260/230
        let shoulder_sigma = 12.0_f64;

        let gauss = |x: f64, mu: f64, sigma: f64| -> f64 {
            let z = (x - mu) / sigma;
            (-0.5 * z * z).exp()
        };

        let mut absorbance = Vec::with_capacity(SPECTRUM_LEN);
        for i in 0..SPECTRUM_LEN {
            let wl = WL_START_NM + (i as f64) * WL_STEP_NM;
            let main = a260_peak * gauss(wl, 260.0, main_sigma);
            let shoulder = shoulder_amp * gauss(wl, 230.0, shoulder_sigma);
            // A touch of protein-ish absorbance near 280 keeps 260/280 realistic
            // without a hard edge; negligible elsewhere.
            let mut a = main + shoulder;
            // Small per-wavelength measurement noise around the true curve.
            a += self.next_noise(0.08);
            // Clamp tiny negatives from noise so the baseline reads ~0.
            if a < 0.0 && wl > 320.0 {
                a = self.next_noise(0.02).abs();
            }
            absorbance.push(a);
        }

        Spectrum::new(absorbance)
    }

    /// Synthesize a *blank re-read*: the reference measured against itself.
    ///
    /// A good blank is flat and near zero (only measurement noise). When
    /// `simulate_bad_blank` is set, a raised, sloping baseline is returned so
    /// [`verify_blank`](Spectrometer::verify_blank) rejects it.
    fn synthesize_blank(&mut self) -> Spectrum {
        let mut absorbance = Vec::with_capacity(SPECTRUM_LEN);
        for i in 0..SPECTRUM_LEN {
            let wl = WL_START_NM + (i as f64) * WL_STEP_NM;
            let mut a = self.next_noise(0.02);
            if self.simulate_bad_blank {
                // A large, wavelength-dependent offset — clearly out of range.
                a += 0.5 + (350.0 - wl).max(0.0) * 0.01;
            }
            absorbance.push(a);
        }
        Spectrum::new(absorbance)
    }
}

impl Default for MockSpectrometer {
    fn default() -> Self {
        Self::new()
    }
}

impl Spectrometer for MockSpectrometer {
    fn info(&self) -> DeviceInfo {
        self.info.clone()
    }

    fn has_blank(&self) -> bool {
        self.has_blank
    }

    fn blank(&mut self) -> Result<(), DeviceError> {
        // A real instrument would capture the buffer intensity spectrum here;
        // the mock just records that a reference now exists. Advance the RNG so
        // subsequent measurements differ from run to run.
        sleep(self.delay);
        let _ = self.next_unit();
        self.has_blank = true;
        Ok(())
    }

    fn verify_blank(&mut self) -> Result<Spectrum, DeviceError> {
        if !self.has_blank {
            return Err(DeviceError::NoBlank);
        }
        // Re-read the reference against itself; a good blank is flat and near
        // zero across the plotted window.
        let reading = self.synthesize_blank();
        let worst = reading
            .points()
            .filter(|(wl, _)| (220.0..=350.0).contains(wl))
            .map(|(_, a)| a.abs())
            .fold(0.0_f64, f64::max);
        if worst > BLANK_MAX_ABS {
            self.has_blank = false;
            return Err(DeviceError::Other(format!(
                "blank reference out of range ({worst:.2} AU > {BLANK_MAX_ABS:.2} AU); reclean the pedestal and re-blank"
            )));
        }
        Ok(reading)
    }

    fn measure(&mut self) -> Result<Spectrum, DeviceError> {
        if !self.has_blank {
            return Err(DeviceError::NoBlank);
        }
        sleep(self.delay);
        self.sample_counter = self.sample_counter.wrapping_add(1);
        Ok(self.synthesize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock with no simulated acquisition delay, so tests don't sleep.
    fn instant() -> MockSpectrometer {
        let mut dev = MockSpectrometer::new();
        dev.set_delay(Duration::ZERO);
        dev
    }

    #[test]
    fn measure_requires_blank() {
        let mut dev = instant();
        assert!(matches!(dev.measure(), Err(DeviceError::NoBlank)));
        dev.blank().unwrap();
        assert!(dev.has_blank());
        let s = dev.measure().unwrap();
        assert_eq!(s.absorbance.len(), SPECTRUM_LEN);
    }

    #[test]
    fn verify_blank_requires_a_blank() {
        let mut dev = instant();
        assert!(matches!(dev.verify_blank(), Err(DeviceError::NoBlank)));
    }

    #[test]
    fn verify_blank_passes_for_a_good_blank() {
        let mut dev = instant();
        dev.blank().unwrap();
        assert!(dev.verify_blank().is_ok());
        // A good blank stays usable afterwards.
        assert!(dev.has_blank());
    }

    #[test]
    fn verify_blank_fails_for_a_bad_blank() {
        let mut dev = instant();
        dev.blank().unwrap();
        dev.set_simulate_bad_blank(true);
        assert!(matches!(dev.verify_blank(), Err(DeviceError::Other(_))));
        // A failed check clears the (unusable) blank.
        assert!(!dev.has_blank());
    }

    #[test]
    fn peak_is_near_260() {
        let mut dev = instant();
        dev.blank().unwrap();
        let s = dev.measure().unwrap();
        let a260 = s.absorbance_at(260.0);
        let a340 = s.absorbance_at(340.0);
        // 260 nm should dominate the baseline.
        assert!(a260 > 5.0, "a260 = {a260}");
        assert!(a340 < 1.0, "a340 = {a340}");
        // Purity ratios in a believable range.
        let ratio = (a260 - a340) / (s.absorbance_at(280.0) - a340);
        assert!((1.5..2.3).contains(&ratio), "260/280 = {ratio}");
    }
}
