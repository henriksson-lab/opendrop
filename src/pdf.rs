//! PDF export of the displayed plot + the full sample table (A4 landscape).
//!
//! GUI-only (behind the `gui` feature) — it is used solely by the File > Export
//! PDF action. Kept free of any Slint types so it stays a pure data → file step.
//!
//! All page geometry is in millimetres as `f32`, matching `printpdf::Mm(f32)`.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use printpdf::{BuiltinFont, Color, IndirectFontRef, Line, Mm, PdfDocument, PdfLayerReference, Point, Rgb};

/// One overlaid spectral trace to draw on the plot.
pub struct PdfTrace {
    /// RGB colour key (matches the on-screen trace + table swatch).
    pub color: (u8, u8, u8),
    /// Legend label, e.g. "#3  my sample".
    pub label: String,
    /// `(wavelength_nm, absorbance)` points, already cropped to the plot window.
    pub points: Vec<(f64, f64)>,
}

/// Everything needed to render the report.
pub struct Report {
    /// Instrument identity line printed under the title.
    pub device_text: String,
    /// Traces currently displayed on the plot.
    pub traces: Vec<PdfTrace>,
    /// Plotted wavelength window `(min_nm, max_nm)`.
    pub x_range: (f64, f64),
    /// Table rows: `[#, id, type, A260, A280, 260/280, 260/230, ng/µL]`.
    pub rows: Vec<Vec<String>>,
}

// A4 landscape, millimetres.
const PAGE_W: f32 = 297.0;
const PAGE_H: f32 = 210.0;

// Plot box (bottom-left origin, mm).
const PLOT_X0: f32 = 22.0;
const PLOT_X1: f32 = 150.0;
const PLOT_Y0: f32 = 118.0;
const PLOT_Y1: f32 = 182.0;

/// Render `report` to a PDF at `path`.
pub fn export(path: &Path, report: &Report) -> anyhow::Result<()> {
    let (doc, page, layer_idx) =
        PdfDocument::new("OpenDrop Report", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let layer = doc.get_page(page).get_layer(layer_idx);
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;

    // --- Title + instrument line ---
    layer.set_fill_color(gray(0.11));
    layer.use_text("OpenDrop — Nucleic Acid Report", 18.0, Mm(15.0), Mm(196.0), &bold);
    layer.set_fill_color(gray(0.4));
    layer.use_text(&report.device_text, 9.0, Mm(15.0), Mm(189.0), &font);

    draw_plot(&layer, report, &font);
    draw_table(&layer, report, &font, &bold);

    doc.save(&mut BufWriter::new(File::create(path)?))
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    Ok(())
}

/// Draw the plot box, axis labels, traces, and legend.
fn draw_plot(layer: &PdfLayerReference, report: &Report, font: &IndirectFontRef) {
    let (xmin, xmax) = report.x_range;

    // Shared Y max across all displayed traces (>= 1.0).
    let y_max = report
        .traces
        .iter()
        .flat_map(|t| t.points.iter().map(|&(_, a)| a))
        .fold(0.0_f64, f64::max);
    let y_max = if y_max <= 0.0 { 1.0 } else { y_max * 1.1 };

    let px = |wl: f64| PLOT_X0 + ((wl - xmin) / (xmax - xmin)) as f32 * (PLOT_X1 - PLOT_X0);
    let py = |a: f64| (PLOT_Y0 + (a / y_max) as f32 * (PLOT_Y1 - PLOT_Y0)).clamp(PLOT_Y0, PLOT_Y1);

    // Axis box.
    layer.set_outline_color(gray(0.7));
    layer.set_outline_thickness(0.5);
    stroke(
        layer,
        &[
            (PLOT_X0, PLOT_Y0),
            (PLOT_X1, PLOT_Y0),
            (PLOT_X1, PLOT_Y1),
            (PLOT_X0, PLOT_Y1),
        ],
        true,
    );

    // Axis labels.
    layer.set_fill_color(gray(0.4));
    layer.use_text("10 mm Absorbance", 8.0, Mm(PLOT_X0), Mm(PLOT_Y1 + 3.0), font);
    layer.use_text("Wavelength (nm)", 8.0, Mm(PLOT_X1 - 26.0), Mm(PLOT_Y0 - 6.0), font);
    layer.use_text(format!("{y_max:.1}"), 8.0, Mm(PLOT_X0 - 8.0), Mm(PLOT_Y1 - 1.5), font);
    layer.use_text("0.0", 8.0, Mm(PLOT_X0 - 6.0), Mm(PLOT_Y0 - 1.0), font);
    for nm in [xmin, (xmin + xmax) / 2.0, xmax] {
        layer.use_text(format!("{nm:.0}"), 8.0, Mm(px(nm) - 3.0), Mm(PLOT_Y0 - 4.0), font);
    }

    // Traces.
    layer.set_outline_thickness(1.0);
    for tr in &report.traces {
        let (r, g, b) = tr.color;
        layer.set_outline_color(rgb(r, g, b));
        let pts: Vec<(f32, f32)> = tr.points.iter().map(|&(wl, a)| (px(wl), py(a))).collect();
        stroke(layer, &pts, false);
    }

    // Legend to the right of the plot.
    let mut ly: f32 = PLOT_Y1 - 2.0;
    for tr in &report.traces {
        let (r, g, b) = tr.color;
        layer.set_outline_color(rgb(r, g, b));
        layer.set_outline_thickness(1.5);
        stroke(layer, &[(PLOT_X1 + 6.0, ly + 1.0), (PLOT_X1 + 12.0, ly + 1.0)], false);
        layer.set_fill_color(gray(0.2));
        layer.use_text(&tr.label, 8.0, Mm(PLOT_X1 + 14.0), Mm(ly), font);
        ly -= 6.0;
    }
}

/// Draw the sample table below the plot.
fn draw_table(layer: &PdfLayerReference, report: &Report, font: &IndirectFontRef, bold: &IndirectFontRef) {
    // Left x of each column (mm).
    const COLS: [f32; 8] = [15.0, 26.0, 74.0, 96.0, 118.0, 140.0, 168.0, 196.0];
    const HEADERS: [&str; 8] = ["#", "Sample ID", "Type", "A260", "A280", "260/280", "260/230", "ng/µL"];
    const ROW_H: f32 = 5.5;
    let mut y: f32 = 100.0;

    layer.set_fill_color(gray(0.11));
    for (i, h) in HEADERS.iter().enumerate() {
        layer.use_text(*h, 9.0, Mm(COLS[i]), Mm(y), bold);
    }
    y -= 1.5;
    layer.set_outline_color(gray(0.7));
    layer.set_outline_thickness(0.4);
    stroke(layer, &[(15.0, y), (PAGE_W - 15.0, y)], false);
    y -= ROW_H;

    layer.set_fill_color(gray(0.15));
    for row in &report.rows {
        if y < 14.0 {
            layer.set_fill_color(gray(0.4));
            layer.use_text("… more rows omitted", 8.0, Mm(15.0), Mm(y + ROW_H - 1.0), font);
            break;
        }
        for (i, cell) in row.iter().enumerate() {
            layer.use_text(cell, 9.0, Mm(COLS[i]), Mm(y), font);
        }
        y -= ROW_H;
    }
}

/// Stroke a polyline (or closed polygon) through `pts` on the current layer.
fn stroke(layer: &PdfLayerReference, pts: &[(f32, f32)], closed: bool) {
    if pts.len() < 2 {
        return;
    }
    let points = pts
        .iter()
        .map(|&(x, y)| (Point::new(Mm(x), Mm(y)), false))
        .collect();
    layer.add_line(Line {
        points,
        is_closed: closed,
    });
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(Rgb::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, None))
}

fn gray(v: f32) -> Color {
    Color::Rgb(Rgb::new(v, v, v, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_writes_a_valid_pdf() {
        let report = Report {
            device_text: "ND-1000 (mock) · MOCK-08708".to_string(),
            traces: vec![PdfTrace {
                color: (0x4f, 0x6e, 0xf7),
                label: "#1  demo".to_string(),
                points: (220..=350)
                    .map(|nm| (nm as f64, ((nm as f64 - 260.0) / 30.0).exp().recip() * 12.0))
                    .collect(),
            }],
            x_range: (220.0, 350.0),
            rows: (1..=3)
                .map(|i| {
                    vec![
                        i.to_string(),
                        format!("Sample {i}"),
                        "dsDNA".to_string(),
                        "12.000".to_string(),
                        "6.500".to_string(),
                        "1.85".to_string(),
                        "2.10".to_string(),
                        "600.0".to_string(),
                    ]
                })
                .collect(),
        };

        let path = std::env::temp_dir().join("opendrop_pdf_export_test.pdf");
        export(&path, &report).expect("export should succeed");

        let bytes = std::fs::read(&path).expect("output file should exist");
        assert!(bytes.starts_with(b"%PDF"), "output should be a PDF");
        assert!(bytes.len() > 500, "PDF should have real content");
        let _ = std::fs::remove_file(&path);
    }
}
