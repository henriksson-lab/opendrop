//! Integration tests against synthetic fixtures matching the documented schema.
//!
//! Fixtures are Windows-1252, tab-delimited, CRLF, with the full 220.0…750.0
//! (531-column) spectrum block:
//! - `Nucleic Acid 2007 06 04.ndj` — no preamble; Blank + 2 Measure rows.
//! - `report_sample.ndv`       — 4-row Data Viewer preamble + 2 Measure rows.

use std::path::PathBuf;

use opendrop::formats::{read_archive, to_string, write_archive, MeasurementType};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn ndj_parses_full_spectrum_and_metadata() {
    let a = read_archive(fixture("Nucleic Acid 2007 06 04.ndj")).unwrap();

    // Module derived from file name.
    assert_eq!(a.module, "Nucleic Acid");
    // No preamble for a bare .ndj.
    assert!(a.preamble.is_empty());
    // 16 metadata + 531 spectrum columns.
    assert_eq!(a.column_order.len(), 16 + 531);
    assert_eq!(a.wavelengths.len(), 531);
    assert_eq!(a.wavelengths.first(), Some(&220.0));
    assert_eq!(a.wavelengths.last(), Some(&750.0));

    assert_eq!(a.rows.len(), 3);

    let blank = &a.rows[0];
    assert_eq!(blank.sample_id(), Some("Blank"));
    assert_eq!(blank.measurement_type(), Some(MeasurementType::Blank));
    assert!(blank.spectrum.absorbance.iter().all(|&v| v == 0.0));
    assert_eq!(blank.spectrum.absorbance.len(), 531);

    let s1 = &a.rows[1];
    assert_eq!(s1.sample_id(), Some("sample-1"));
    assert_eq!(s1.measurement_type(), Some(MeasurementType::Measure));
    assert_eq!(s1.get("A260"), Some("0.5"));
    // Spectrum peak at 260 nm (index 40).
    assert_eq!(s1.spectrum.absorbance_at(260.0), 0.5);
    assert_eq!(s1.spectrum.absorbance_at(280.0), 0.27);
}

#[test]
fn ndv_captures_four_row_preamble() {
    let a = read_archive(fixture("report_sample.ndv")).unwrap();
    assert_eq!(a.preamble.len(), 4);
    assert_eq!(a.preamble[0], "Test Type\tNucleic Acid");
    assert_eq!(a.preamble[2], "Report Name\tDemo Report");
    assert_eq!(a.rows.len(), 2);
    assert_eq!(a.column_order.len(), 16 + 531);
}

#[test]
fn ndj_byte_stable_and_round_trips() {
    let path = fixture("Nucleic Acid 2007 06 04.ndj");
    let a = read_archive(&path).unwrap();

    // Read -> write reproduces the original bytes exactly.
    let original_bytes = std::fs::read(&path).unwrap();
    let written_bytes = opendrop::formats::encode_windows_1252(&to_string(&a));
    assert_eq!(
        written_bytes, original_bytes,
        "read->write must be byte-stable"
    );

    // parse -> write -> parse -> equal (string API; `module` is a file-name
    // property, not stored in the file, so we compare via strings).
    let b = opendrop::formats::parse(&to_string(&a)).unwrap();
    let mut a_no_module = a.clone();
    a_no_module.module = String::new();
    assert_eq!(a_no_module, b);

    // The file API also round-trips through disk.
    let tmp = std::env::temp_dir().join("nanodrop_ndj_roundtrip.ndj");
    write_archive(&a, &tmp).unwrap();
    let c = read_archive(&tmp).unwrap();
    assert_eq!(a.rows, c.rows);
    assert_eq!(a.column_order, c.column_order);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn ndv_byte_stable_round_trip() {
    let path = fixture("report_sample.ndv");
    let a = read_archive(&path).unwrap();
    let original_bytes = std::fs::read(&path).unwrap();
    let written_bytes = opendrop::formats::encode_windows_1252(&to_string(&a));
    assert_eq!(
        written_bytes, original_bytes,
        ".ndv read->write must be byte-stable"
    );
}
