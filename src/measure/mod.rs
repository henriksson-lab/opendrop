//! Measurement data model and math (concentrations, purity ratios, protein
//! A280). Pure and GUI-free, unit-tested against the constants recovered from
//! the original ND-1000 software (see `docs/`).
//!
//! - [`spectrum`] — the shared [`Spectrum`] type and wavelength axis.
//! - [`calc`] — Nucleic Acid calculations.
//! - [`protein`] — Protein A280 calculations (E1%, mg/mL).
//! - [`constants`] — recovered numeric constants, each cited to `docs/`.

pub mod calc;
pub mod constants;
pub mod protein;
pub mod spectrum;

pub use spectrum::{Spectrum, SPECTRUM_LEN, WL_END_NM, WL_START_NM, WL_STEP_NM};
