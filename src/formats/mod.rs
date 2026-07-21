//! `nanodrop-fileformats` — readers and writers for NanoDrop ND-1000 archive
//! files (tab-delimited `.ndj`/`.ndt`/`.ndv`, Windows-1252).
//!
//! See `docs/file_formats.md` for the reverse-engineered schema. This crate
//! parses a file into a lossless [`Archive`] (verbatim `column_order` and
//! `preamble`, raw metadata cells) carrying one [`Measurement`] per row, each
//! with the shared [`crate::measure::Spectrum`], and writes it back so a
//! read → write round-trip is byte-stable for files we produced.
//!
//! # What is CONFIRMED vs ASSUMED
//! - CONFIRMED (from the shipped binary/manual): Windows-1252 encoding; the
//!   tab-delimited layout; the exact 16 Nucleic Acid metadata columns; the
//!   `220.0…750.0` @ 1 nm spectrum block; classifying columns as
//!   spectrum-vs-metadata by whether the header parses as a float; the `.ndv`
//!   4-row preamble; that the data-start offset is version-dependent (hence we
//!   scan for the `Sample ID` header rather than hard-coding an offset).
//! - ASSUMED (pending a real sample file): the exact `.ndj` preamble contents
//!   and version line; per-column numeric formatting/padding for a *byte-for-
//!   byte* match with the original software's output; the metadata column order
//!   of non–Nucleic-Acid modules; deriving [`Archive::module`] from the file
//!   name. These do not affect losslessness of files this crate produces.
//!
//! # Example
//! ```no_run
//! use opendrop::formats::{read_archive, write_archive};
//! let archive = read_archive("Nucleic Acid 2007 06 04.ndj")?;
//! for row in &archive.rows {
//!     println!("{:?}: A260 cell = {:?}", row.sample_id(), row.get("A260"));
//! }
//! write_archive(&archive, "copy.ndj")?;
//! # Ok::<(), opendrop::formats::FormatError>(())
//! ```

mod error;
mod model;
mod reader;
mod writer;

pub use error::FormatError;
pub use model::{
    format_wavelength, is_wavelength_header, Archive, LineEnding, Measurement, MeasurementType,
    NUCLEIC_ACID_METADATA_COLUMNS,
};
pub use reader::{decode_windows_1252, module_from_filename};
pub use writer::{encode_windows_1252, try_encode_windows_1252};

use std::path::Path;

/// Parse a decoded archive string into an [`Archive`]. The `module` field is
/// left empty (there is no file name); set it yourself if known.
pub fn parse(text: &str) -> Result<Archive, FormatError> {
    reader::parse(text)
}

/// Serialize an [`Archive`] to a `String` in the tab-delimited layout.
pub fn to_string(archive: &Archive) -> String {
    writer::to_string(archive)
}

/// Read and parse an archive file, decoding it as Windows-1252. Sets
/// [`Archive::module`] from the file name (best effort).
pub fn read_archive<P: AsRef<Path>>(path: P) -> Result<Archive, FormatError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| FormatError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let text = decode_windows_1252(&bytes);
    let mut archive = reader::parse(&text)?;
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        archive.module = module_from_filename(stem);
    }
    Ok(archive)
}

/// Serialize an [`Archive`] and write it to `path`, encoded as Windows-1252.
pub fn write_archive<P: AsRef<Path>>(archive: &Archive, path: P) -> Result<(), FormatError> {
    let path = path.as_ref();
    writer::validate_archive(archive)?;
    let bytes = try_encode_windows_1252(&to_string(archive))?;
    std::fs::write(path, bytes).map_err(|source| FormatError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny 2-wavelength archive string (no preamble) for focused unit tests.
    fn tiny() -> String {
        // Header: 3 metadata cols + 2 spectrum cols, CRLF, trailing newline.
        let mut s = String::new();
        s.push_str("Sample ID\tMeasurement Type\t260/280\t260.0\t280.0\r\n");
        s.push_str("Blank\tBlank\t\t0\t0\r\n");
        s.push_str("s1\tMeasure\t1.85\t0.5\t0.27\r\n");
        s
    }

    #[test]
    fn header_detection_skips_preamble() {
        let mut s = String::from("Report Name\tDemo\r\nspacer\r\n");
        s.push_str(&tiny());
        let a = parse(&s).unwrap();
        assert_eq!(a.preamble, vec!["Report Name\tDemo", "spacer"]);
        assert_eq!(a.column_order[0], "Sample ID");
        assert_eq!(a.rows.len(), 2);
    }

    #[test]
    fn metadata_spectrum_split() {
        let a = parse(&tiny()).unwrap();
        assert_eq!(a.wavelengths, vec![260.0, 280.0]);
        assert_eq!(
            a.metadata_columns().collect::<Vec<_>>(),
            vec!["Sample ID", "Measurement Type", "260/280"]
        );
        let s1 = &a.rows[1];
        assert_eq!(s1.sample_id(), Some("s1"));
        assert_eq!(s1.measurement_type(), Some(MeasurementType::Measure));
        assert_eq!(s1.get("260/280"), Some("1.85"));
        assert_eq!(s1.spectrum.absorbance, vec![0.5, 0.27]);
        assert_eq!(a.rows[0].measurement_type(), Some(MeasurementType::Blank));
    }

    #[test]
    fn windows_1252_decode() {
        // 0xB5 is 'µ' in CP-1252 (would be invalid as lone UTF-8). Put it in a
        // metadata cell and confirm it decodes to U+00B5.
        let mut bytes = b"Sample ID\t260.0\r\n".to_vec();
        bytes.extend_from_slice(&[0xB5, b'g']); // "µg" as a sample id
        bytes.extend_from_slice(b"\t0\r\n");
        let text = decode_windows_1252(&bytes);
        let a = parse(&text).unwrap();
        assert_eq!(a.rows[0].sample_id(), Some("\u{00B5}g"));
    }

    #[test]
    fn round_trip_string_stable() {
        let original = tiny();
        let a = parse(&original).unwrap();
        let written = to_string(&a);
        // Byte-stable for a file we could have produced.
        assert_eq!(written, original);
        // parse -> write -> parse -> equal
        let b = parse(&written).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn missing_header_errors() {
        assert!(matches!(
            parse("no header here\r\njust data\r\n"),
            Err(FormatError::MissingHeader)
        ));
    }

    #[test]
    fn column_count_mismatch_errors() {
        let bad = "Sample ID\t260.0\r\nonlyone\r\n";
        assert!(matches!(
            parse(bad),
            Err(FormatError::ColumnCountMismatch {
                row: 1,
                expected: 2,
                found: 1
            })
        ));
    }

    #[test]
    fn module_from_filename_strips_date_and_version() {
        assert_eq!(
            module_from_filename("Nucleic Acid 2007 06 04"),
            "Nucleic Acid"
        );
        assert_eq!(
            module_from_filename("Nucleic Acid 2005 09 09 v3.2"),
            "Nucleic Acid"
        );
        assert_eq!(module_from_filename("UV-Vis"), "UV-Vis");
    }

    #[test]
    fn nucleic_acid_header_has_531_spectrum_cols() {
        let h = Archive::nucleic_acid_header();
        assert_eq!(h.len(), 16 + 531);
        assert_eq!(h[16], "220.0");
        assert_eq!(h[h.len() - 1], "750.0");
    }

    #[test]
    fn non_finite_headers_are_metadata_not_wavelengths() {
        assert!(!is_wavelength_header("NaN"));
        assert!(!is_wavelength_header("inf"));
        assert!(is_wavelength_header("260.0"));
    }
}
