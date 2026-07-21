//! OpenDrop application entry point.
//!
//! Wires the Slint UI (`AppWindow`, see `ui/app.slint`) to a controller that
//! owns a `Box<dyn Spectrometer>` (the mock backend by default). It handles the
//! Blank / Measure / Re-blank callbacks, turns each `Spectrum` into an SVG path
//! for the graph, and computes the Nucleic Acid readouts for display.

use std::cell::RefCell;
use std::rc::Rc;

use opendrop::device::mock::MockSpectrometer;
use opendrop::device::{DeviceError, Spectrometer};
use opendrop::measure::calc::SampleType;
use opendrop::measure::Spectrum;
use slint::{Color, SharedString};

slint::include_modules!();

/// Wavelength window shown on the Nucleic Acid graph (nm).
const PLOT_MIN_NM: f64 = 220.0;
const PLOT_MAX_NM: f64 = 350.0;
/// Viewbox extents used by the Slint `Path` (see `SpectrumPlot` in the UI).
const VIEW_W: f64 = 1000.0;
const VIEW_H: f64 = 1000.0;

/// Mutable application state shared with the Slint callbacks.
struct AppState {
    /// The active spectrometer backend (mock by default).
    device: Box<dyn Spectrometer>,
    /// The most recently measured sample spectrum, if any.
    last: Option<Spectrum>,
    /// Running sample counter (increments on each Measure).
    sample_count: i32,
}

fn main() -> anyhow::Result<()> {
    let ui = AppWindow::new()?;

    let device: Box<dyn Spectrometer> = Box::new(MockSpectrometer::new());
    let firmware = device.info().config.clone();
    let state = Rc::new(RefCell::new(AppState {
        device,
        last: None,
        sample_count: 0,
    }));

    // Initial screen state: flat baseline, no readouts yet.
    ui.set_firmware_text(firmware.into());
    ui.set_plot_commands(flat_baseline_commands().into());
    ui.set_y_max_label("1.0".into());
    clear_readouts(&ui);

    // --- Blank (F3): store a reference, clear the screen to a flat line. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_blank(move || {
            let ui = ui.unwrap();
            let mut st = state.borrow_mut();
            match st.device.blank() {
                Ok(()) => {
                    st.last = None;
                    ui.set_has_blank(true);
                    ui.set_plot_commands(flat_baseline_commands().into());
                    ui.set_y_max_label("1.0".into());
                    clear_readouts(&ui);
                    ui.set_status_text(
                        "Blank measurement complete. Load a sample and click Measure.".into(),
                    );
                }
                Err(e) => ui.set_status_text(format!("Blank failed: {e}").into()),
            }
        });
    }

    // --- Measure (F1): read a sample against the stored blank. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_measure(move || {
            let ui = ui.unwrap();
            let mut st = state.borrow_mut();
            match st.device.measure() {
                Ok(spectrum) => {
                    st.sample_count += 1;
                    let count = st.sample_count;
                    st.last = Some(spectrum);
                    ui.set_sample_count(count);
                    ui.set_status_text("Measurement complete.".into());
                    drop(st);
                    refresh(&ui, &state);
                }
                Err(DeviceError::NoBlank) => {
                    ui.set_status_text("Make a BLANK measurement first.".into());
                }
                Err(e) => ui.set_status_text(format!("Measure failed: {e}").into()),
            }
        });
    }

    // --- Re-blank (F2): new reference, re-referenced against displayed sample. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_reblank(move || {
            let ui = ui.unwrap();
            let blank_result = {
                let mut st = state.borrow_mut();
                st.device.blank()
            };
            match blank_result {
                Ok(()) => {
                    ui.set_has_blank(true);
                    ui.set_status_text("New blank recorded. Existing spectrum unchanged.".into());
                    refresh(&ui, &state);
                }
                Err(e) => ui.set_status_text(format!("Re-blank failed: {e}").into()),
            }
        });
    }

    // --- Sample Type changed: recompute concentration / colour key. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_sample_type_changed(move |_idx| {
            let ui = ui.unwrap();
            refresh(&ui, &state);
        });
    }

    // --- Cursor wavelength changed: update the λ / Abs readout. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_cursor_changed(move |_v| {
            let ui = ui.unwrap();
            update_cursor(&ui, &state);
        });
    }

    // --- Exit from the Main Menu. ---
    {
        let ui = ui.as_weak();
        ui.unwrap().on_request_exit(move || {
            let _ = ui;
            let _ = slint::quit_event_loop();
        });
    }

    ui.run()?;
    Ok(())
}

/// Map the Sample Type dropdown index to a core `SampleType`.
fn sample_type_for(index: i32) -> SampleType {
    match index {
        0 => SampleType::DsDna,        // DNA-50
        1 => SampleType::Rna,          // RNA-40
        2 => SampleType::SsDna,        // ssDNA-33
        _ => SampleType::Custom(30.0), // Other (default user constant)
    }
}

/// Colour key for the ng/µL box and the Sample Type swatch (mirrors theme.slint).
fn color_for(index: i32) -> Color {
    match index {
        0 => Color::from_rgb_u8(0x45, 0xb5, 0x45), // green
        1 => Color::from_rgb_u8(0xe0, 0xa0, 0x20), // gold
        2 => Color::from_rgb_u8(0x40, 0xb0, 0xc8), // cyan
        _ => Color::from_rgb_u8(0xb8, 0xb0, 0xd8), // lavender
    }
}

/// Recompute every readout + the graph from the last measured spectrum and the
/// currently-selected sample type, and push them into the UI.
fn refresh(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let idx = ui.get_sample_type_index();
    ui.set_conc_color(color_for(idx));

    let Some(spectrum) = st.last.as_ref() else {
        clear_readouts(ui);
        return;
    };

    // All readout math lives in `nanodrop-core` (unit-tested against the
    // constants recovered from the original software). The core baseline-
    // corrects at 340 nm and returns 0.0 for undefined purity ratios.
    let sample_type = sample_type_for(idx);
    let r = opendrop::measure::calc::nucleic_acid(spectrum, sample_type);

    ui.set_a260_text(format!("{:.3}", r.a260).into());
    ui.set_a280_text(format!("{:.3}", r.a280).into());
    ui.set_ratio_280_text(fmt_ratio(r.ratio_260_280).into());
    ui.set_ratio_230_text(fmt_ratio(r.ratio_260_230).into());
    ui.set_conc_text(format!("{:.1}", r.concentration_ng_per_ul).into());

    let (commands, ymax) = build_plot_commands(spectrum);
    ui.set_plot_commands(commands.into());
    ui.set_y_max_label(format!("{ymax:.1}").into());

    drop(st);
    update_cursor(ui, state);
}

/// Update just the λ / Abs cursor readout from the last spectrum.
fn update_cursor(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let nm: f64 = ui.get_cursor_nm().parse().unwrap_or(230.0);
    let text = match st.last.as_ref() {
        Some(spec) => format!("{:.3}", spec.absorbance_at(nm)),
        None => "—".to_string(),
    };
    ui.set_cursor_abs_text(text.into());
}

/// Reset all numeric readouts to placeholders.
fn clear_readouts(ui: &AppWindow) {
    let dash: SharedString = "—".into();
    ui.set_a260_text(dash.clone());
    ui.set_a280_text(dash.clone());
    ui.set_ratio_280_text(dash.clone());
    ui.set_ratio_230_text(dash.clone());
    ui.set_conc_text(dash.clone());
    ui.set_cursor_abs_text(dash);
    ui.set_conc_color(color_for(ui.get_sample_type_index()));
}

/// Format a purity ratio. `nanodrop-core` returns `0.0` for an undefined ratio
/// (zero denominator); show that — and any non-finite value — as a blank dash,
/// matching the original's empty readout.
fn fmt_ratio(v: f64) -> String {
    if v.is_finite() && v != 0.0 {
        format!("{v:.2}")
    } else {
        "—".to_string()
    }
}

/// Build the SVG path (in a 0..1000 viewbox) tracing the 220–350 nm region of a
/// spectrum, auto-scaling Y. Returns the path string and the Y-axis max used.
fn build_plot_commands(spectrum: &Spectrum) -> (String, f64) {
    // Collect (wavelength, absorbance) within the plotted window.
    let pts: Vec<(f64, f64)> = spectrum
        .points()
        .filter(|(wl, _)| (PLOT_MIN_NM..=PLOT_MAX_NM).contains(wl))
        .collect();
    if pts.is_empty() {
        return (flat_baseline_commands(), 1.0);
    }

    let max_a = pts.iter().fold(f64::MIN, |m, &(_, a)| m.max(a));
    let min_a = pts.iter().fold(f64::MAX, |m, &(_, a)| m.min(a));

    // Auto-scale: top a little above the peak (min 1.0), bottom at min(0, data).
    let top = if max_a <= 0.0 { 1.0 } else { max_a * 1.1 };
    let bottom = min_a.min(0.0);
    let span = (top - bottom).max(1e-6);

    let mut cmds = String::with_capacity(pts.len() * 14);
    for (i, &(wl, a)) in pts.iter().enumerate() {
        let x = (wl - PLOT_MIN_NM) / (PLOT_MAX_NM - PLOT_MIN_NM) * VIEW_W;
        let y = (1.0 - (a - bottom) / span) * VIEW_H;
        let y = y.clamp(0.0, VIEW_H);
        cmds.push_str(if i == 0 { "M " } else { "L " });
        cmds.push_str(&format!("{x:.1} {y:.1} "));
    }
    (cmds, top)
}

/// A flat baseline near the bottom of the plot (used for a fresh blank / reset).
fn flat_baseline_commands() -> String {
    format!("M 0 {y:.1} L {w:.1} {y:.1}", y = VIEW_H - 4.0, w = VIEW_W)
}
