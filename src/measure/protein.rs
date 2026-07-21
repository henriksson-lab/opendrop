//! Measurement math for the **Protein A280** module.
//!
//! The A280 module quantifies purified protein directly from its aromatic
//! absorbance at 280 nm, using a per-protein extinction coefficient expressed
//! as **E1%** — the absorbance of a 1% (w/v) = 10 mg/mL solution at a 1 cm path.
//! As with the Nucleic Acid module the spectrum is baseline-corrected at 340 nm
//! and reported at the 10 mm-equivalent path length (already normalized in
//! [`Spectrum`]).
//!
//! Constants and formulas are recovered from `docs/labview_strings_findings.md`
//! §2 and `docs/original_gui_research.md` §6.6.

use crate::measure::constants;
use crate::measure::spectrum::Spectrum;

/// Protein A280 sample type and its E1% extinction coefficient.
///
/// E1% is the A280 of a 1% (10 mg/mL) solution at 1 cm. The generic
/// "1 Abs = 1 mg/mL" option corresponds to E1% = 10.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ProteinSampleType {
    /// Generic assumption: 1 Abs (A280) = 1 mg/mL, i.e. E1% = 10. (Default.)
    #[default]
    OneAbsPerMgMl,
    /// Bovine serum albumin, E1% = 6.67.
    Bsa,
    /// Immunoglobulin G, E1% = 13.6.
    IgG,
    /// Lysozyme, E1% = 26.4.
    Lysozyme,
    /// User-supplied E1% (absorbance of a 10 mg/mL solution at 1 cm).
    CustomE1(f64),
    /// User-supplied molar extinction `epsilon` (1/(M·cm)) and molecular weight
    /// `mw`. The E1% is derived as `epsilon / (10 · mw)`, matching the original
    /// `Calculate E1% from eps and MW.vi`.
    Custom {
        /// Molar extinction coefficient ε, in 1/(M·cm).
        epsilon: f64,
        /// Molecular weight (same unit basis as the original's MW field).
        mw: f64,
    },
}

impl ProteinSampleType {
    /// The E1% extinction coefficient for this sample type.
    ///
    /// For [`ProteinSampleType::Custom`] this is `epsilon / (10 · mw)`; if
    /// `mw` is zero the result is `0.0` (guarded), which downstream yields a
    /// concentration of `0.0`.
    pub fn e1_percent(&self) -> f64 {
        match self {
            ProteinSampleType::OneAbsPerMgMl => constants::E1PCT_GENERIC,
            ProteinSampleType::Bsa => constants::E1PCT_BSA,
            ProteinSampleType::IgG => constants::E1PCT_IGG,
            ProteinSampleType::Lysozyme => constants::E1PCT_LYSOZYME,
            ProteinSampleType::CustomE1(e1) => *e1,
            ProteinSampleType::Custom { epsilon, mw } => {
                if *mw != 0.0 {
                    epsilon / (10.0 * mw)
                } else {
                    0.0
                }
            }
        }
    }

    /// The factor that converts a baseline-corrected A280 into mg/mL:
    /// `10 / E1%`. (A280 · factor = concentration in mg/mL.)
    ///
    /// Returns `0.0` when E1% is zero (guarded divide-by-zero).
    pub fn factor(&self) -> f64 {
        let e1 = self.e1_percent();
        if e1 != 0.0 {
            10.0 / e1
        } else {
            0.0
        }
    }
}

/// Result of a Protein A280 measurement (10 mm-normalized, baseline-corrected
/// at 340 nm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProteinA280Result {
    /// Absorbance at 280 nm, minus the 340 nm baseline.
    pub a280: f64,
    /// Absorbance at 260 nm, minus the 340 nm baseline (shown alongside A280).
    pub a260: f64,
    /// Raw absorbance at 340 nm (the baseline that was subtracted).
    pub a340: f64,
    /// The E1% used for this measurement.
    pub e1_percent: f64,
    /// 260/280 purity ratio (`0.0` if A280 is zero).
    pub ratio_260_280: f64,
    /// Protein concentration in mg/mL: `a280 · (10 / e1_percent)`.
    pub concentration_mg_per_ml: f64,
}

/// Compute the Protein A280 readouts from a 10 mm-normalized [`Spectrum`].
///
/// Baseline-corrects at 340 nm, then derives A280 (and A260 for the ratio) and
/// the concentration in mg/mL for `sample_type`. Concentration is
/// `A280 · 10 / E1%`; a zero E1% (e.g. a degenerate custom input) yields `0.0`.
pub fn a280(spectrum: &Spectrum, sample_type: ProteinSampleType) -> ProteinA280Result {
    let a340 = spectrum.absorbance_at(constants::WL_BASELINE_NM);
    let a280 = spectrum.absorbance_at(constants::WL_A280_NM) - a340;
    let a260 = spectrum.absorbance_at(constants::WL_A260_NM) - a340;

    let e1_percent = sample_type.e1_percent();
    let concentration_mg_per_ml = a280 * sample_type.factor();

    let ratio_260_280 = if a280 != 0.0 { a260 / a280 } else { 0.0 };

    ProteinA280Result {
        a280,
        a260,
        a340,
        e1_percent,
        ratio_260_280,
        concentration_mg_per_ml,
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
    fn e1_percent_constants() {
        assert_eq!(ProteinSampleType::OneAbsPerMgMl.e1_percent(), 10.0);
        assert_eq!(ProteinSampleType::Bsa.e1_percent(), 6.67);
        assert_eq!(ProteinSampleType::IgG.e1_percent(), 13.6);
        assert_eq!(ProteinSampleType::Lysozyme.e1_percent(), 26.4);
    }

    #[test]
    fn default_is_one_abs_per_mg_ml() {
        assert_eq!(
            ProteinSampleType::default(),
            ProteinSampleType::OneAbsPerMgMl
        );
    }

    #[test]
    fn one_abs_equals_one_mg_per_ml() {
        // A280 = 1.0, A340 = 0.0 -> 1.0 mg/mL for the generic type.
        let s = spectrum_with(&[(280.0, 1.0)]);
        let r = a280(&s, ProteinSampleType::OneAbsPerMgMl);
        assert!((r.a280 - 1.0).abs() < 1e-12);
        assert!((r.concentration_mg_per_ml - 1.0).abs() < 1e-12);
    }

    #[test]
    fn bsa_concentration() {
        // A280 = 1.0 -> 10 / 6.67 = 1.4993 mg/mL.
        let s = spectrum_with(&[(280.0, 1.0)]);
        let r = a280(&s, ProteinSampleType::Bsa);
        assert!((r.concentration_mg_per_ml - 10.0 / 6.67).abs() < 1e-12);
    }

    #[test]
    fn igg_and_lysozyme_concentration() {
        let s = spectrum_with(&[(280.0, 2.0)]);
        let igg = a280(&s, ProteinSampleType::IgG);
        assert!((igg.concentration_mg_per_ml - 2.0 * 10.0 / 13.6).abs() < 1e-12);
        let lyso = a280(&s, ProteinSampleType::Lysozyme);
        assert!((lyso.concentration_mg_per_ml - 2.0 * 10.0 / 26.4).abs() < 1e-12);
    }

    #[test]
    fn baseline_subtraction() {
        // A280 raw = 1.3, A340 = 0.3 -> corrected 1.0 -> 1.0 mg/mL generic.
        let s = spectrum_with(&[(280.0, 1.3), (340.0, 0.3)]);
        let r = a280(&s, ProteinSampleType::OneAbsPerMgMl);
        assert!((r.a340 - 0.3).abs() < 1e-12);
        assert!((r.a280 - 1.0).abs() < 1e-12);
        assert!((r.concentration_mg_per_ml - 1.0).abs() < 1e-12);
    }

    #[test]
    fn custom_e1_percent() {
        let s = spectrum_with(&[(280.0, 1.0)]);
        let r = a280(&s, ProteinSampleType::CustomE1(20.0));
        assert!((r.concentration_mg_per_ml - 0.5).abs() < 1e-12);
    }

    #[test]
    fn custom_epsilon_mw() {
        // E1% = eps / (10 * mw) = 66700 / (10 * 66430) ~= 0.10041.
        let t = ProteinSampleType::Custom {
            epsilon: 66700.0,
            mw: 66430.0,
        };
        let expected_e1 = 66700.0 / (10.0 * 66430.0);
        assert!((t.e1_percent() - expected_e1).abs() < 1e-12);
        let s = spectrum_with(&[(280.0, 1.0)]);
        let r = a280(&s, t);
        assert!((r.concentration_mg_per_ml - 10.0 / expected_e1).abs() < 1e-9);
    }

    #[test]
    fn zero_mw_is_guarded() {
        let t = ProteinSampleType::Custom {
            epsilon: 1000.0,
            mw: 0.0,
        };
        assert_eq!(t.e1_percent(), 0.0);
        assert_eq!(t.factor(), 0.0);
        let s = spectrum_with(&[(280.0, 1.0)]);
        let r = a280(&s, t);
        assert_eq!(r.concentration_mg_per_ml, 0.0);
    }

    #[test]
    fn ratio_guarded_when_a280_zero() {
        let s = spectrum_with(&[(260.0, 1.0)]);
        let r = a280(&s, ProteinSampleType::OneAbsPerMgMl);
        assert_eq!(r.ratio_260_280, 0.0);
    }
}
