//! The spectral data model shared across the whole workspace.
//!
//! The ND-1000 records absorbance from 220 nm to 750 nm in 1.0 nm steps
//! (531 points), normalized to a 10 mm path length. See
//! `docs/original_gui_research.md` and `docs/file_formats.md`.

/// First wavelength recorded by the ND-1000, in nanometres.
pub const WL_START_NM: f64 = 220.0;
/// Last wavelength recorded by the ND-1000, in nanometres.
pub const WL_END_NM: f64 = 750.0;
/// Wavelength step, in nanometres.
pub const WL_STEP_NM: f64 = 1.0;
/// Number of points in a full ND-1000 spectrum (220..=750 @ 1 nm).
pub const SPECTRUM_LEN: usize = 531;

/// An absorbance spectrum normalized to a 10 mm path length.
///
/// `absorbance[i]` corresponds to wavelength `WL_START_NM + i * WL_STEP_NM`.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    /// Absorbance values (AU), one per wavelength, 10 mm-normalized.
    pub absorbance: Vec<f64>,
}

impl Spectrum {
    /// Build a spectrum from a full-length absorbance vector.
    pub fn new(absorbance: Vec<f64>) -> Self {
        Self { absorbance }
    }

    /// A flat zero spectrum of the canonical ND-1000 length.
    pub fn zeros() -> Self {
        Self {
            absorbance: vec![0.0; SPECTRUM_LEN],
        }
    }

    /// The wavelength (nm) at sample index `i`.
    pub fn wavelength_at(i: usize) -> f64 {
        WL_START_NM + (i as f64) * WL_STEP_NM
    }

    /// The nearest sample index for a wavelength in nm, clamped to range.
    pub fn index_of_wavelength(nm: f64) -> usize {
        let raw = ((nm - WL_START_NM) / WL_STEP_NM).round();
        raw.clamp(0.0, (SPECTRUM_LEN - 1) as f64) as usize
    }

    /// Iterator over `(wavelength_nm, absorbance)` pairs.
    pub fn points(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.absorbance
            .iter()
            .enumerate()
            .map(|(i, &a)| (Self::wavelength_at(i), a))
    }

    /// Absorbance at the nearest recorded wavelength to `nm`.
    ///
    /// Returns `0.0` if the spectrum does not contain that wavelength.
    pub fn absorbance_at(&self, nm: f64) -> f64 {
        self.absorbance
            .get(Self::index_of_wavelength(nm))
            .copied()
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absorbance_at_missing_wavelength_is_zero() {
        let spectrum = Spectrum::new(vec![0.1, 0.2]);

        assert_eq!(spectrum.absorbance_at(WL_START_NM), 0.1);
        assert_eq!(spectrum.absorbance_at(WL_END_NM), 0.0);
    }

    #[test]
    fn absorbance_at_empty_spectrum_is_zero() {
        let spectrum = Spectrum::new(Vec::new());

        assert_eq!(spectrum.absorbance_at(260.0), 0.0);
    }
}
