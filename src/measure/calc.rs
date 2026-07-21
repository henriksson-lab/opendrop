//! Measurement calculations for the Nucleic Acid module (and shared helpers).
//!
//! This file defines the STABLE public contract (types + function signatures)
//! that the GUI depends on. The `nanodrop-core` worker implements the bodies
//! and adds unit tests against the constants in `docs/labview_strings_findings.md`
//! and `docs/original_gui_research.md`. Do not change these signatures without
//! coordinating — the GUI calls them directly.

use crate::measure::constants;
use crate::measure::spectrum::Spectrum;

/// Nucleic-acid sample type and its A260 → concentration constant (ng/µL per
/// 1.0 AU at 10 mm). Values recovered from the original software.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SampleType {
    /// Double-stranded DNA, constant 50.
    #[default]
    DsDna,
    /// Single-stranded DNA, constant 33.
    SsDna,
    /// RNA, constant 40.
    Rna,
    /// User-defined constant (ng/µL per AU), UI range ~15–150.
    Custom(f64),
}

impl SampleType {
    /// The A260 → concentration constant (ng/µL per AU) for this sample type.
    pub fn constant(&self) -> f64 {
        match self {
            SampleType::DsDna => 50.0,
            SampleType::SsDna => 33.0,
            SampleType::Rna => 40.0,
            SampleType::Custom(c) => *c,
        }
    }
}

/// Result of a Nucleic Acid measurement (all absorbances 10 mm-normalized,
/// baseline-corrected at 340 nm as the original does).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NucleicAcidResult {
    /// Absorbance at 260 nm, minus the 340 nm baseline.
    pub a260: f64,
    /// Absorbance at 280 nm, minus the 340 nm baseline.
    pub a280: f64,
    /// Absorbance at 230 nm, minus the 340 nm baseline.
    pub a230: f64,
    /// Raw absorbance at 340 nm (the baseline that was subtracted).
    pub a340: f64,
    /// 260/280 purity ratio.
    pub ratio_260_280: f64,
    /// 260/230 purity ratio.
    pub ratio_260_230: f64,
    /// Concentration in ng/µL: `a260 * sample_type.constant()`.
    pub concentration_ng_per_ul: f64,
}

/// Compute the Nucleic Acid readouts from a 10 mm-normalized [`Spectrum`].
///
/// Baseline-corrects at 340 nm (subtracting A340 from A260/A280/A230), then
/// derives the purity ratios and the concentration for `sample_type`:
///
/// - `260/280 = A260 / A280`, `260/230 = A260 / A230`
/// - `ng/µL = A260 · sample_type.constant()`
///
/// Purity ratios guard against divide-by-zero by returning `0.0` when the
/// denominator is zero (matching the "empty readout" behaviour of the
/// original blank screen rather than propagating a NaN/∞).
pub fn nucleic_acid(spectrum: &Spectrum, sample_type: SampleType) -> NucleicAcidResult {
    // Baseline is the raw absorbance at 340 nm; it is subtracted from the
    // analytical wavelengths (docs/original_gui_research.md §3.3 / §6.4).
    let a340 = spectrum.absorbance_at(constants::WL_BASELINE_NM);
    let a260 = spectrum.absorbance_at(constants::WL_A260_NM) - a340;
    let a280 = spectrum.absorbance_at(constants::WL_A280_NM) - a340;
    let a230 = spectrum.absorbance_at(constants::WL_A230_NM) - a340;

    let ratio_260_280 = if a280 != 0.0 { a260 / a280 } else { 0.0 };
    let ratio_260_230 = if a230 != 0.0 { a260 / a230 } else { 0.0 };

    let concentration_ng_per_ul = a260 * sample_type.constant();

    NucleicAcidResult {
        a260,
        a280,
        a230,
        a340,
        ratio_260_280,
        ratio_260_230,
        concentration_ng_per_ul,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::spectrum::Spectrum;

    /// Build a zero spectrum with specific absorbances set at given wavelengths.
    fn spectrum_with(points: &[(f64, f64)]) -> Spectrum {
        let mut s = Spectrum::zeros();
        for &(nm, a) in points {
            s.absorbance[Spectrum::index_of_wavelength(nm)] = a;
        }
        s
    }

    #[test]
    fn sample_type_constants() {
        assert_eq!(SampleType::DsDna.constant(), 50.0);
        assert_eq!(SampleType::Rna.constant(), 40.0);
        assert_eq!(SampleType::SsDna.constant(), 33.0);
        assert_eq!(SampleType::Custom(72.0).constant(), 72.0);
        assert_eq!(SampleType::default(), SampleType::DsDna);
    }

    #[test]
    fn concentration_for_each_type_at_a260_one() {
        // A260 = 1.0, A340 = 0.0 -> 50 / 40 / 33 ng/µL.
        let s = spectrum_with(&[(260.0, 1.0)]);

        let dsdna = nucleic_acid(&s, SampleType::DsDna);
        assert!((dsdna.a260 - 1.0).abs() < 1e-12);
        assert!((dsdna.concentration_ng_per_ul - 50.0).abs() < 1e-12);

        let rna = nucleic_acid(&s, SampleType::Rna);
        assert!((rna.concentration_ng_per_ul - 40.0).abs() < 1e-12);

        let ssdna = nucleic_acid(&s, SampleType::SsDna);
        assert!((ssdna.concentration_ng_per_ul - 33.0).abs() < 1e-12);
    }

    #[test]
    fn baseline_subtraction_at_340() {
        // Raw A260 = 1.2 with A340 = 0.2 -> corrected A260 = 1.0 -> 50 ng/µL.
        let s = spectrum_with(&[(260.0, 1.2), (280.0, 0.7), (340.0, 0.2)]);
        let r = nucleic_acid(&s, SampleType::DsDna);
        assert!((r.a340 - 0.2).abs() < 1e-12);
        assert!((r.a260 - 1.0).abs() < 1e-12);
        assert!((r.a280 - 0.5).abs() < 1e-12);
        assert!((r.concentration_ng_per_ul - 50.0).abs() < 1e-12);
    }

    #[test]
    fn purity_ratios() {
        // Corrected A260=1.0, A280=0.5, A230=0.4 (A340=0) -> 2.0 and 2.5.
        let s = spectrum_with(&[(260.0, 1.0), (280.0, 0.5), (230.0, 0.4)]);
        let r = nucleic_acid(&s, SampleType::DsDna);
        assert!((r.ratio_260_280 - 2.0).abs() < 1e-12);
        assert!((r.ratio_260_230 - 2.5).abs() < 1e-12);
    }

    #[test]
    fn ratios_guarded_against_divide_by_zero() {
        // Only A260 present: A280 and A230 corrected are zero -> ratios 0.0.
        let s = spectrum_with(&[(260.0, 1.0)]);
        let r = nucleic_acid(&s, SampleType::DsDna);
        assert_eq!(r.ratio_260_280, 0.0);
        assert_eq!(r.ratio_260_230, 0.0);
    }

    #[test]
    fn custom_constant() {
        let s = spectrum_with(&[(260.0, 2.0)]);
        let r = nucleic_acid(&s, SampleType::Custom(45.0));
        assert!((r.concentration_ng_per_ul - 90.0).abs() < 1e-12);
    }

    #[test]
    fn realistic_curve_matches_manual_example() {
        // Manual §5-1 example: A260 = 35.019, A280 = 20.576 -> 260/280 = 1.70;
        // dsDNA conc = 35.019 * 50 = 1750.95 ng/µL. (The screenshot's 1950.9 is
        // a different sample; here we check our arithmetic is self-consistent.)
        let s = spectrum_with(&[(260.0, 35.019), (280.0, 20.576)]);
        let r = nucleic_acid(&s, SampleType::DsDna);
        assert!((r.ratio_260_280 - (35.019 / 20.576)).abs() < 1e-9);
        assert!((r.concentration_ng_per_ul - 35.019 * 50.0).abs() < 1e-9);
    }
}
