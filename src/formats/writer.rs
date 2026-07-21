//! Writer: serialize an [`Archive`] back to the tab-delimited format.

use crate::formats::error::FormatError;
use crate::formats::model::{is_wavelength_header, Archive};

/// Encode a string as Windows-1252 bytes for writing.
///
/// Characters outside CP-1252 are replaced (encoding_rs uses `?`); archive data
/// is Win ANSI so this is normally lossless.
pub fn encode_windows_1252(text: &str) -> Vec<u8> {
    encoding_rs::WINDOWS_1252.encode(text).0.into_owned()
}

/// Encode a string as Windows-1252, reporting lossy replacement.
pub fn try_encode_windows_1252(text: &str) -> Result<Vec<u8>, FormatError> {
    let (bytes, _, had_errors) = encoding_rs::WINDOWS_1252.encode(text);
    if had_errors {
        Err(FormatError::UnencodableText)
    } else {
        Ok(bytes.into_owned())
    }
}

/// Validate invariants that parsed archives always satisfy before writing.
pub fn validate_archive(archive: &Archive) -> Result<(), FormatError> {
    let metadata_columns = archive
        .column_order
        .iter()
        .filter(|header| !is_wavelength_header(header))
        .count();
    let spectrum_columns = archive
        .column_order
        .iter()
        .filter(|header| is_wavelength_header(header))
        .count();

    if spectrum_columns != archive.wavelengths.len() {
        return Err(FormatError::InvalidArchive {
            reason: format!(
                "header has {spectrum_columns} spectrum columns but wavelengths has {} entries",
                archive.wavelengths.len()
            ),
        });
    }

    for (idx, row) in archive.rows.iter().enumerate() {
        let row_no = idx + 1;
        if row.metadata.len() != metadata_columns {
            return Err(FormatError::InvalidArchive {
                reason: format!(
                    "row {row_no} has {} metadata cells but header has {metadata_columns}",
                    row.metadata.len()
                ),
            });
        }
        if row.spectrum.absorbance.len() != spectrum_columns {
            return Err(FormatError::InvalidArchive {
                reason: format!(
                    "row {row_no} has {} spectrum values but header has {spectrum_columns}",
                    row.spectrum.absorbance.len()
                ),
            });
        }
    }

    Ok(())
}

/// Format an absorbance value for a spectrum cell.
///
/// Uses the shortest representation that round-trips the `f64` exactly (Rust's
/// default float formatting), so `parse(write(x)) == x` and repeated writes are
/// byte-stable.
fn format_absorbance(v: f64) -> String {
    format!("{v}")
}

/// Serialize an [`Archive`] to a `String` in the tab-delimited NanoDrop layout.
///
/// Reproduces the preamble and header verbatim from `column_order`; each row is
/// rebuilt by walking `column_order`, pulling metadata cells in order and
/// spectrum cells from the sample spectrum. Byte-stable for files we produced.
pub fn to_string(archive: &Archive) -> String {
    let eol = archive.line_ending.as_str();

    let mut lines: Vec<String> =
        Vec::with_capacity(archive.preamble.len() + archive.rows.len() + 1);
    lines.extend(archive.preamble.iter().cloned());
    lines.push(archive.column_order.join("\t"));

    for row in &archive.rows {
        let mut cells: Vec<String> = Vec::with_capacity(archive.column_order.len());
        let mut meta_idx = 0usize;
        let mut spec_idx = 0usize;
        for header in &archive.column_order {
            if is_wavelength_header(header) {
                let v = row
                    .spectrum
                    .absorbance
                    .get(spec_idx)
                    .copied()
                    .unwrap_or(0.0);
                spec_idx += 1;
                cells.push(format_absorbance(v));
            } else {
                let value = row
                    .metadata
                    .get(meta_idx)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                meta_idx += 1;
                cells.push(value);
            }
        }
        lines.push(cells.join("\t"));
    }

    let mut out = lines.join(eol);
    if archive.trailing_newline {
        out.push_str(eol);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::model::{LineEnding, Measurement};
    use crate::measure::Spectrum;

    fn archive() -> Archive {
        Archive {
            module: String::new(),
            preamble: Vec::new(),
            column_order: vec!["Sample ID".into(), "260.0".into(), "280.0".into()],
            wavelengths: vec![260.0, 280.0],
            rows: vec![Measurement {
                metadata: vec![("Sample ID".into(), "s1".into())],
                spectrum: Spectrum::new(vec![0.5, 0.25]),
            }],
            line_ending: LineEnding::Crlf,
            trailing_newline: true,
        }
    }

    #[test]
    fn validate_archive_rejects_missing_spectrum_values() {
        let mut archive = archive();
        archive.rows[0].spectrum = Spectrum::new(vec![0.5]);

        assert!(matches!(
            validate_archive(&archive),
            Err(FormatError::InvalidArchive { .. })
        ));
    }

    #[test]
    fn validate_archive_rejects_missing_metadata_values() {
        let mut archive = archive();
        archive.rows[0].metadata.clear();

        assert!(matches!(
            validate_archive(&archive),
            Err(FormatError::InvalidArchive { .. })
        ));
    }

    #[test]
    fn try_encode_windows_1252_reports_lossy_text() {
        assert!(matches!(
            try_encode_windows_1252("sample \u{1f600}"),
            Err(FormatError::UnencodableText)
        ));
    }
}
