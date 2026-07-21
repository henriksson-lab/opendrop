//! The spectrometer abstraction for OpenDrop.
//!
//! Callers talk to hardware only through the [`Spectrometer`] trait, so the
//! synthetic [`mock`] backend and the future real [`usb2000`] backend are fully
//! swappable.

use crate::measure::Spectrum;

/// Errors surfaced by a spectrometer backend.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("no spectrometer found")]
    NotFound,
    #[error("device is not connected")]
    NotConnected,
    #[error("must record a blank reference before measuring")]
    NoBlank,
    #[error("device communication error: {0}")]
    Io(String),
    #[error("{0}")]
    Other(String),
}

/// Static information about a connected instrument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Model string, e.g. "ND-1000".
    pub model: String,
    /// Instrument serial number.
    pub serial: String,
    /// Firmware / config identifier, if known.
    pub config: String,
}

/// A spectrophotometer capable of blank + sample absorbance reads.
///
/// The workflow mirrors the ND-1000: record a blank reference, then each
/// [`measure`](Spectrometer::measure) returns absorbance relative to it.
pub trait Spectrometer {
    /// Information about the connected instrument.
    fn info(&self) -> DeviceInfo;

    /// Whether a usable blank reference is currently held.
    fn has_blank(&self) -> bool;

    /// Record a blank reference (the buffer/solvent baseline).
    fn blank(&mut self) -> Result<(), DeviceError>;

    /// Measure a sample, returning a 10 mm-normalized absorbance [`Spectrum`].
    ///
    /// Returns [`DeviceError::NoBlank`] if no blank has been recorded.
    fn measure(&mut self) -> Result<Spectrum, DeviceError>;
}

pub mod mock;
pub mod usb2000;
