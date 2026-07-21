//! Mock spectrometer backend — synthesizes plausible spectra with no hardware.
//!
//! The mock mimics the ND-1000 workflow: a [`blank`](MockSpectrometer::blank)
//! must be recorded before [`measure`](MockSpectrometer::measure) will return a
//! sample. Each `measure` produces a DNA-like absorbance curve (10 mm-normalized)
//! peaking near 260 nm with a 230 nm shoulder and a near-zero 340 nm baseline,
//! plus a little per-call noise. Randomness comes from a small internal LCG (a
//! deterministic counter-based generator), so no `rand`/`Math::random` is needed
//! and results still vary from call to call.

use crate::measure::spectrum::{Spectrum, SPECTRUM_LEN, WL_START_NM, WL_STEP_NM};

use crate::device::{DeviceError, DeviceInfo, Spectrometer};

/// A synthetic spectrometer that fabricates plausible nucleic-acid spectra.
pub struct MockSpectrometer {
    info: DeviceInfo,
    /// Whether a blank reference has been recorded this session.
    has_blank: bool,
    /// State of the internal LCG used for per-call variation.
    rng_state: u64,
    /// Monotonic counter of samples measured (drives amplitude variation).
    sample_counter: u64,
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
        }
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
        let _ = self.next_unit();
        self.has_blank = true;
        Ok(())
    }

    fn measure(&mut self) -> Result<Spectrum, DeviceError> {
        if !self.has_blank {
            return Err(DeviceError::NoBlank);
        }
        self.sample_counter = self.sample_counter.wrapping_add(1);
        Ok(self.synthesize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_requires_blank() {
        let mut dev = MockSpectrometer::new();
        assert!(matches!(dev.measure(), Err(DeviceError::NoBlank)));
        dev.blank().unwrap();
        assert!(dev.has_blank());
        let s = dev.measure().unwrap();
        assert_eq!(s.absorbance.len(), SPECTRUM_LEN);
    }

    #[test]
    fn peak_is_near_260() {
        let mut dev = MockSpectrometer::new();
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
