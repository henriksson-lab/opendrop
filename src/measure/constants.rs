//! Recovered numeric constants from the original NanoDrop ND-1000 software.
//!
//! Every value here is cited to the reverse-engineering notes in `docs/`
//! (`labview_strings_findings.md` and `original_gui_research.md`). Centralizing
//! them keeps the measurement math auditable against the original.

// --- Analytical wavelengths (nm) -------------------------------------------

/// Nucleic-acid quantitation peak: A260.
///
/// Source: `docs/original_gui_research.md` §3.2 / §6.4 ("A-260 10 mm path").
pub const WL_A260_NM: f64 = 260.0;

/// Protein / aromatic peak: A280.
///
/// Source: `docs/original_gui_research.md` §3.2 / §6.6 ("A-280 10 mm path").
pub const WL_A280_NM: f64 = 280.0;

/// Secondary purity wavelength: A230 (used for the 260/230 ratio).
///
/// Source: `docs/original_gui_research.md` §6.5.
pub const WL_A230_NM: f64 = 230.0;

/// Baseline-normalization wavelength.
///
/// The original auto-sets the baseline to the absorbance at 340 nm ("340 nm
/// normalization"), which is subtracted from A260/A280/A230.
/// Source: `docs/original_gui_research.md` §3.3 / §6.4;
/// `docs/labview_strings_findings.md` §3 ("340 nm normalization").
pub const WL_BASELINE_NM: f64 = 340.0;

// --- Nucleic-acid A260 -> concentration constants (ng/µL per AU) ------------

/// Double-stranded DNA constant (DNA-50).
///
/// Source: `docs/labview_strings_findings.md` §2 / `original_gui_research.md` §6.4.
pub const DSDNA_CONSTANT: f64 = 50.0;

/// RNA constant (RNA-40).
pub const RNA_CONSTANT: f64 = 40.0;

/// Single-stranded DNA constant (ssDNA-33).
pub const SSDNA_CONSTANT: f64 = 33.0;

// --- Protein A280 extinction coefficients (E1%, L·g⁻¹·cm⁻¹) ----------------
//
// E1% is the absorbance at 280 nm of a 1% (w/v) = 10 mg/mL solution at a 1 cm
// path length. The generic "1 Abs = 1 mg/mL" assumption corresponds to E1% = 10
// (10 mg/mL -> A280 = 10 -> 1 mg/mL -> A280 = 1). Concentration is therefore
// `mg/mL = A280 · 10 / E1%`.
//
// Source: `docs/labview_strings_findings.md` §2 ("Protein A280 sample-type enum").

/// Generic "1 Abs = 1 mg/mL" E1% (i.e. E1% = 10).
pub const E1PCT_GENERIC: f64 = 10.0;

/// Bovine serum albumin (BSA) E1%.
pub const E1PCT_BSA: f64 = 6.67;

/// Immunoglobulin G (IgG) E1%.
pub const E1PCT_IGG: f64 = 13.6;

/// Lysozyme E1%.
pub const E1PCT_LYSOZYME: f64 = 26.4;
