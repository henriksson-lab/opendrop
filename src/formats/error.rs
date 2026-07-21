//! Error type for the file-format reader/writer.

/// Errors that can occur while reading or writing a NanoDrop archive.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// Underlying filesystem error (from [`crate::read_archive`] /
    /// [`crate::write_archive`]).
    #[error("I/O error for {path:?}: {source}")]
    Io {
        /// The path that was being read or written.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// No column-header row (a line whose first tab field is `Sample ID`) was
    /// found anywhere in the file.
    #[error("no column-header row (a line beginning with `Sample ID`) was found")]
    MissingHeader,

    /// The header contained no wavelength (spectrum) columns.
    #[error("the header has no spectrum (wavelength) columns")]
    NoSpectrumColumns,

    /// A data row had a different number of tab-separated fields than the
    /// header.
    #[error("data row {row} has {found} fields but the header has {expected}")]
    ColumnCountMismatch {
        /// 1-based data-row number (not counting preamble/header).
        row: usize,
        /// Number of columns in the header.
        expected: usize,
        /// Number of fields found in this row.
        found: usize,
    },

    /// A spectrum cell could not be parsed as a floating-point absorbance.
    #[error("data row {row}, column `{column}`: cannot parse absorbance {value:?}")]
    BadSpectrumValue {
        /// 1-based data-row number.
        row: usize,
        /// The wavelength column header, e.g. `260.0`.
        column: String,
        /// The offending cell contents.
        value: String,
    },

    /// An in-memory archive violates invariants required for safe writing.
    #[error("invalid archive: {reason}")]
    InvalidArchive {
        /// Human-readable description of the violated invariant.
        reason: String,
    },

    /// The archive contains text that cannot be represented in Windows-1252.
    #[error("archive contains characters that cannot be encoded as Windows-1252")]
    UnencodableText,
}
