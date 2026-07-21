//! In-memory data model for a NanoDrop ND-1000 tab-delimited archive.
//!
//! The model is designed for **lossless round-trip**: [`Archive::column_order`]
//! and [`Archive::preamble`] are kept verbatim, and metadata cells are stored as
//! raw strings so re-serialization reproduces the original bytes for files we
//! produced. See `docs/file_formats.md` §2 (schema) and §5 (strategy).

use crate::measure::Spectrum;

/// The verbatim metadata column headers of the **Nucleic Acid** module, in
/// order, as recovered from the ND-1000 V3.8.1 binary.
///
/// CONFIRMED: extracted verbatim from the EXE (`docs/labview_strings_findings.md`
/// §4, `docs/file_formats.md` §2a). Other modules have different metadata
/// columns and are ASSUMED-pending-a-real-file.
pub const NUCLEIC_ACID_METADATA_COLUMNS: [&str; 16] = [
    "Sample ID",
    "User ID",
    "Date",
    "Time",
    "ng/ul",
    "A260",
    "A280",
    "260/280",
    "260/230",
    "Constant",
    "Cursor Pos.",
    "Cursor abs.",
    "340 raw",
    "Measurement Type",
    "Serial #",
    "Config.",
];

/// Returns `true` if a column header denotes a spectrum (wavelength) column.
///
/// A column is a spectrum column iff its trimmed header parses as a float, e.g.
/// `220.0`. None of the documented metadata headers (`Sample ID`, `260/280`,
/// `340 raw`, …) parse as a bare float, so this cleanly separates the two.
/// CONFIRMED strategy per `docs/file_formats.md` §5.
pub fn is_wavelength_header(header: &str) -> bool {
    header.trim().parse::<f64>().is_ok_and(f64::is_finite)
}

/// The kind of measurement in a row's `Measurement Type` column.
///
/// CONFIRMED semantics: `docs/file_formats.md` §2a (manual §15 Note 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasurementType {
    /// Normal measurement using the stored blank.
    Measure,
    /// The initial blank measurement.
    Blank,
    /// Re-analysis against a new blank.
    Reblank,
    /// Any value we don't recognise, preserved verbatim.
    Other(String),
}

impl MeasurementType {
    /// Classify a raw `Measurement Type` cell.
    pub fn from_cell(s: &str) -> Self {
        match s.trim() {
            "Measure" => Self::Measure,
            "Blank" => Self::Blank,
            "Reblank" | "Re-Blank" | "Re-blank" => Self::Reblank,
            other => Self::Other(other.to_string()),
        }
    }
}

/// One measurement row: ordered metadata cells plus the per-sample spectrum.
///
/// `metadata` holds the non-spectrum columns as ordered `(header, value)` pairs
/// in the same order they appear in [`Archive::column_order`]. Values are kept
/// as raw strings (never re-parsed/re-formatted) so unknown or module-specific
/// columns round-trip verbatim.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// Ordered metadata cells `(column header, raw value)` — order-preserving
    /// map implemented as a `Vec` per the task brief.
    pub metadata: Vec<(String, String)>,
    /// The full per-sample absorbance spectrum, aligned to
    /// [`Archive::wavelengths`].
    pub spectrum: Spectrum,
}

impl Measurement {
    /// Look up a metadata cell by its exact column header.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.metadata
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// `Sample ID` cell, if present.
    pub fn sample_id(&self) -> Option<&str> {
        self.get("Sample ID")
    }

    /// `User ID` cell, if present.
    pub fn user_id(&self) -> Option<&str> {
        self.get("User ID")
    }

    /// `Date` cell (locale-formatted string, kept raw), if present.
    pub fn date(&self) -> Option<&str> {
        self.get("Date")
    }

    /// `Time` cell (locale-formatted string, kept raw), if present.
    pub fn time(&self) -> Option<&str> {
        self.get("Time")
    }

    /// `Serial #` cell, if present.
    pub fn serial(&self) -> Option<&str> {
        self.get("Serial #")
    }

    /// `Config.` cell, if present.
    pub fn config(&self) -> Option<&str> {
        self.get("Config.")
    }

    /// The classified `Measurement Type`, if that column is present.
    pub fn measurement_type(&self) -> Option<MeasurementType> {
        self.get("Measurement Type").map(MeasurementType::from_cell)
    }
}

/// Line-ending style, preserved for byte-stable round-trips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// `\r\n` — the style written by the original Windows software.
    Crlf,
    /// `\n`.
    Lf,
}

impl LineEnding {
    /// The literal string for this line ending.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Crlf => "\r\n",
            LineEnding::Lf => "\n",
        }
    }
}

/// A parsed NanoDrop archive: one module's tab-delimited file.
#[derive(Debug, Clone, PartialEq)]
pub struct Archive {
    /// Application module name, e.g. `"Nucleic Acid"`.
    ///
    /// ASSUMED source: derived from the file name for `.ndj`/`.ndt` (the file
    /// itself does not name its module); empty when parsed from a bare string.
    pub module: String,
    /// Raw preamble lines above the header row, verbatim (no line endings).
    /// Empty for a bare `.ndj`; the 4 report rows for `.ndv`.
    pub preamble: Vec<String>,
    /// The full column header row, verbatim and in order (metadata + spectrum).
    /// Used to rewrite the header byte-for-byte.
    pub column_order: Vec<String>,
    /// The spectrum wavelength axis (nm), parsed from the spectrum columns of
    /// `column_order`, in column order.
    pub wavelengths: Vec<f64>,
    /// The measurement rows.
    pub rows: Vec<Measurement>,
    /// Line ending to emit when writing.
    pub line_ending: LineEnding,
    /// Whether the file ends with a trailing newline.
    pub trailing_newline: bool,
}

impl Archive {
    /// The metadata column headers (non-spectrum), in order.
    pub fn metadata_columns(&self) -> impl Iterator<Item = &str> {
        self.column_order
            .iter()
            .filter(|h| !is_wavelength_header(h))
            .map(String::as_str)
    }

    /// Build the canonical **Nucleic Acid** header (16 metadata columns followed
    /// by `220.0 … 750.0`) using the shared core wavelength axis. Handy for
    /// constructing archives programmatically and for tests.
    pub fn nucleic_acid_header() -> Vec<String> {
        let mut cols: Vec<String> = NUCLEIC_ACID_METADATA_COLUMNS
            .iter()
            .map(|s| s.to_string())
            .collect();
        for i in 0..crate::measure::SPECTRUM_LEN {
            cols.push(format_wavelength(Spectrum::wavelength_at(i)));
        }
        cols
    }
}

/// Format a wavelength for a column header, e.g. `220.0`.
pub fn format_wavelength(nm: f64) -> String {
    format!("{nm:.1}")
}
