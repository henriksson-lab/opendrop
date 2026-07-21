//! Reader for tab-delimited NanoDrop archives (`.ndj`/`.ndt`/`.ndv`).

use crate::measure::Spectrum;

use crate::formats::error::FormatError;
use crate::formats::model::{is_wavelength_header, Archive, LineEnding, Measurement};

/// Decode raw file bytes as Windows-1252 (CP-1252 / Win ANSI).
///
/// CONFIRMED: LabVIEW writes Win ANSI, not UTF-8 (`docs/file_formats.md` §1/§5).
/// Windows-1252 decoding is total (every byte maps), so this never fails.
pub fn decode_windows_1252(bytes: &[u8]) -> String {
    encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()
}

/// Parse a decoded archive string into an [`Archive`].
///
/// The header row is located by scanning for the line whose first tab field is
/// `Sample ID`; everything above it is captured as preamble. This handles the
/// version-dependent data-start offset (`.ndv`'s 4-row preamble and any `.ndj`
/// version line) without hard-coding an offset — CONFIRMED strategy,
/// `docs/file_formats.md` §5.
pub fn parse(text: &str) -> Result<Archive, FormatError> {
    let line_ending = if text.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };

    // Split on '\n' and strip a trailing '\r' from each line. A final empty
    // element indicates a trailing newline.
    let mut raw: Vec<&str> = text.split('\n').collect();
    let trailing_newline = matches!(raw.last(), Some(&"")) && raw.len() > 1;
    if trailing_newline {
        raw.pop();
    }
    let lines: Vec<&str> = raw
        .into_iter()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();

    // Locate the header row: first line whose first field trims to "Sample ID".
    let header_idx = lines
        .iter()
        .position(|l| l.split('\t').next().map(str::trim) == Some("Sample ID"))
        .ok_or(FormatError::MissingHeader)?;

    let preamble: Vec<String> = lines[..header_idx].iter().map(|s| s.to_string()).collect();

    let column_order: Vec<String> = lines[header_idx]
        .split('\t')
        .map(|s| s.to_string())
        .collect();

    // Wavelength axis from the spectrum columns.
    let wavelengths: Vec<f64> = column_order
        .iter()
        .filter(|h| is_wavelength_header(h))
        .map(|h| {
            h.trim()
                .parse::<f64>()
                .expect("checked by is_wavelength_header")
        })
        .collect();
    if wavelengths.is_empty() {
        return Err(FormatError::NoSpectrumColumns);
    }

    // Parse data rows (skip blank lines, which we never emit ourselves).
    let mut rows = Vec::new();
    let mut data_row_no = 0usize;
    for line in &lines[header_idx + 1..] {
        if line.is_empty() {
            continue;
        }
        data_row_no += 1;

        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != column_order.len() {
            return Err(FormatError::ColumnCountMismatch {
                row: data_row_no,
                expected: column_order.len(),
                found: fields.len(),
            });
        }

        let mut metadata = Vec::new();
        let mut absorbance = Vec::with_capacity(wavelengths.len());
        for (header, cell) in column_order.iter().zip(fields.iter()) {
            if is_wavelength_header(header) {
                let value =
                    cell.trim()
                        .parse::<f64>()
                        .map_err(|_| FormatError::BadSpectrumValue {
                            row: data_row_no,
                            column: header.clone(),
                            value: (*cell).to_string(),
                        })?;
                absorbance.push(value);
            } else {
                metadata.push((header.clone(), (*cell).to_string()));
            }
        }

        rows.push(Measurement {
            metadata,
            spectrum: Spectrum::new(absorbance),
        });
    }

    Ok(Archive {
        module: String::new(),
        preamble,
        column_order,
        wavelengths,
        rows,
        line_ending,
        trailing_newline,
    })
}

/// Best-effort module name from an archive file name, e.g.
/// `Nucleic Acid 2007 06 04.ndj` -> `Nucleic Acid`.
///
/// ASSUMED: strips a trailing `YYYY MM DD` date and any `vX.Y` version token
/// (`docs/file_formats.md` §1 naming). The file contents do not name the module.
pub fn module_from_filename(stem: &str) -> String {
    let mut tokens: Vec<&str> = stem.split_whitespace().collect();
    while let Some(last) = tokens.last() {
        let is_number = last.chars().all(|c| c.is_ascii_digit()) && !last.is_empty();
        let is_version = last.starts_with('v')
            && last[1..].chars().all(|c| c.is_ascii_digit() || c == '.')
            && last.len() > 1;
        if is_number || is_version {
            tokens.pop();
        } else {
            break;
        }
    }
    tokens.join(" ")
}
