//! OpenDrop — an open-source reimplementation of the NanoDrop ND-1000 software.
//!
//! The crate is usable as a **pure library**: [`measure`] (data model + math),
//! [`device`] (the [`Spectrometer`](device::Spectrometer) abstraction + mock
//! backend), and [`formats`] (NanoDrop archive readers/writers) carry no GUI
//! dependency. The desktop GUI and the `opendrop` binary live behind the
//! default-on `gui` feature — disable default features to depend on OpenDrop as
//! a library without pulling in Slint:
//!
//! ```toml
//! opendrop = { version = "0.1", default-features = false }
//! ```

pub mod device;
pub mod formats;
pub mod measure;

pub use measure::Spectrum;
