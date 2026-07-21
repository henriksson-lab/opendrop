//! OpenDrop application entry point.
//!
//! Single-window controller: owns a `Box<dyn Spectrometer>` and a list of
//! `Sample`s, and drives the Slint UI (`AppWindow`, see `ui/app.slint`).
//!
//! Workflow: record a blank (verified on the spot; a green check / red cross on
//! the Blank/Re-blank buttons reflects the result). "New sample" creates an
//! *empty* row (named via a dialog) and selects it. A plain row click selects
//! only that row; Shift-click extends the selection. "Measure" is enabled only
//! with exactly one row selected and fills it (asking before overwriting
//! existing data). "Next" advances the selection, creating a new row past the
//! end. Blank/measure acquisitions are simulated (~1 s) behind a busy overlay.
//! The table is the single source of truth for the readouts; selecting rows
//! overlays their spectra on the plot. File > Export PDF renders the displayed
//! plot plus the full table.

mod pdf;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use opendrop::device::mock::MockSpectrometer;
use opendrop::device::{DeviceError, Spectrometer};
use opendrop::measure::calc::{nucleic_acid, NucleicAcidResult, SampleType};
use opendrop::measure::Spectrum;
use slint::{Color, ModelRc, Timer, VecModel};

slint::include_modules!();

/// Wavelength window shown on the plot (nm).
const PLOT_MIN_NM: f64 = 220.0;
const PLOT_MAX_NM: f64 = 350.0;
/// Viewbox extents used by the Slint `Path` (see `SpectrumPlot` in the UI).
const VIEW_W: f64 = 1000.0;
const VIEW_H: f64 = 1000.0;

/// Per-sample categorical colour key. Mirrors `Theme.series` in `ui/theme.slint`
/// so a sample's table swatch and its plot trace always match.
const PALETTE: [(u8, u8, u8); 8] = [
    (0x4f, 0x6e, 0xf7), // indigo
    (0x10, 0xb9, 0x81), // emerald
    (0xf5, 0x9e, 0x0b), // amber
    (0xef, 0x44, 0x44), // red
    (0x8b, 0x5c, 0xf6), // violet
    (0x06, 0xb6, 0xd4), // cyan
    (0xec, 0x48, 0x99), // pink
    (0x84, 0xcc, 0x16), // lime
];

/// A measured spectrum and its cached Nucleic Acid readouts.
struct Filled {
    /// Sample-type index used at measure time (0 dsDNA, 1 RNA, 2 ssDNA, 3 Other).
    type_index: i32,
    spectrum: Spectrum,
    result: NucleicAcidResult,
}

/// One sample row. `filled` is `None` until the sample has been measured, so a
/// freshly created sample shows as an empty row awaiting a Measure.
struct Sample {
    /// Monotonic sample number (also the legend/table `#` and colour key).
    number: i32,
    /// User-supplied identifier.
    id: String,
    /// Measurement data, or `None` while the sample is still empty.
    filled: Option<Filled>,
    /// Whether this sample is currently overlaid on the plot.
    selected: bool,
}

impl Sample {
    fn color(&self) -> (u8, u8, u8) {
        PALETTE[((self.number - 1).rem_euclid(PALETTE.len() as i32)) as usize]
    }
}

/// Mutable application state shared with the Slint callbacks.
struct AppState {
    device: Box<dyn Spectrometer>,
    samples: Vec<Sample>,
    /// Ever-increasing sample counter (never reused, so colours stay stable).
    next_number: i32,
    /// The sample number that Measure writes into (the "current" sample).
    current: Option<i32>,
    /// Transient blank re-read shown on the plot right after Blank/Re-blank.
    /// Any user action that changes what should be plotted (selecting a row,
    /// measuring, adding/removing samples) clears it. When `Some`, it is what
    /// the plot shows, in place of the selected samples.
    blank_preview: Option<Spectrum>,
}

impl AppState {
    /// Index of the current sample, if it still exists.
    fn current_index(&self) -> Option<usize> {
        let n = self.current?;
        self.samples.iter().position(|s| s.number == n)
    }

    /// Index of the sole selected sample, or `None` if zero or more than one is
    /// selected. Measure writes exactly one sample, so it needs this.
    fn single_selected_index(&self) -> Option<usize> {
        let mut selected = self
            .samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.selected)
            .map(|(i, _)| i);
        let first = selected.next()?;
        selected.next().is_none().then_some(first)
    }

    /// Advance the selection to the sample after the current one — selecting the
    /// first row when nothing is selected, or creating a fresh empty row past the
    /// end. Leaves exactly one row selected. Returns a status message. Drives
    /// both the "Next" button and the post-blank bootstrap.
    fn advance_selection(&mut self) -> String {
        let len = self.samples.len();
        let target = match self.current_index() {
            Some(i) if i + 1 < len => Some(i + 1),
            Some(_) => None,            // at the end — create a new one
            None if len > 0 => Some(0), // nothing selected — start at the top
            None => None,
        };
        match target {
            Some(i) => {
                for (j, s) in self.samples.iter_mut().enumerate() {
                    s.selected = j == i;
                }
                let n = self.samples[i].number;
                self.current = Some(n);
                format!("Selected sample #{n}.")
            }
            None => {
                let number = self.next_number;
                self.next_number += 1;
                for s in &mut self.samples {
                    s.selected = false;
                }
                self.samples.push(Sample {
                    number,
                    id: format!("Sample {number}"),
                    filled: None,
                    selected: true,
                });
                self.current = Some(number);
                format!("Created sample #{number}. Click Measure to fill it in.")
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let ui = AppWindow::new()?;

    let device: Box<dyn Spectrometer> = Box::new(MockSpectrometer::new());
    let info = device.info();
    let has_blank = device.has_blank();
    ui.set_device_text(format!("{} · {} · {}", info.model, info.serial, info.config).into());
    ui.set_version(env!("CARGO_PKG_VERSION").into());

    let state = Rc::new(RefCell::new(AppState {
        device,
        samples: Vec::new(),
        next_number: 1,
        current: None,
        blank_preview: None,
    }));

    ui.set_has_blank(has_blank);

    // Open populated with a few representative demo samples.
    seed_demo(&state);
    ui.set_status_text(if state.borrow().samples.is_empty() {
        "Click Blank to record a reference, then Measure.".into()
    } else {
        "Loaded demo samples. Select rows to overlay them on the plot, or Blank to start a run."
            .into()
    });

    refresh(&ui, &state);

    // --- Blank: record + verify a reference. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_blank(move || {
            perform_blank(&ui.unwrap(), &state, false);
        });
    }

    // --- Re-blank: record + verify a fresh reference without adding a sample. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_reblank(move || {
            perform_blank(&ui.unwrap(), &state, true);
        });
    }

    // --- New sample: open the naming dialog with a sensible default. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_request_new_sample(move || {
            let ui = ui.unwrap();
            let default = format!("Sample {}", state.borrow().next_number);
            ui.set_new_sample_name(default.into());
            ui.set_new_sample_open(true);
        });
    }

    // --- Create sample: append an empty, selected row and make it current. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_create_sample(move |name| {
            let ui = ui.unwrap();
            let number = {
                let mut st = state.borrow_mut();
                st.blank_preview = None;
                let number = st.next_number;
                st.next_number += 1;
                let name = name.trim();
                let id = if name.is_empty() {
                    format!("Sample {number}")
                } else {
                    name.to_string()
                };
                for s in &mut st.samples {
                    s.selected = false;
                }
                st.samples.push(Sample {
                    number,
                    id,
                    filled: None,
                    selected: true,
                });
                st.current = Some(number);
                number
            };
            ui.set_new_sample_open(false);
            ui.set_status_text(
                format!("Created sample #{number}. Click Measure to fill it in.").into(),
            );
            refresh(&ui, &state);
        });
    }

    // --- Add constant: open the dialog prefilled with the current value. ---
    {
        let ui = ui.as_weak();
        ui.unwrap().on_request_custom(move || {
            let ui = ui.unwrap();
            ui.set_custom_input(ui.get_custom_constant());
            ui.set_custom_open(true);
        });
    }

    // --- Set constant: validate, store, and activate the Custom type. ---
    {
        let ui = ui.as_weak();
        ui.unwrap().on_set_custom(move |value| {
            let ui = ui.unwrap();
            match value.trim().parse::<f64>() {
                Ok(c) if c > 0.0 => {
                    ui.set_custom_constant(format!("{c}").into());
                    ui.set_sample_type_index(3);
                    ui.set_custom_open(false);
                    ui.set_status_text(format!("Custom constant set to {c} ng/µL·AU.").into());
                }
                _ => ui.set_status_text("Enter a positive number for the constant.".into()),
            }
        });
    }

    // --- Measure: fill the selected sample, confirming any overwrite first. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_measure(move || {
            let ui = ui.unwrap();
            // Measure is only enabled with exactly one sample selected.
            let (number, already_filled) = {
                let st = state.borrow();
                match st.single_selected_index() {
                    Some(i) => (st.samples[i].number, st.samples[i].filled.is_some()),
                    None => {
                        ui.set_status_text("Select a single sample to measure.".into());
                        return;
                    }
                }
            };
            if already_filled {
                // Ask before clobbering existing data.
                ui.set_confirm_text(
                    format!("Sample #{number} already has data. Overwrite it?").into(),
                );
                ui.set_confirm_overwrite_open(true);
            } else {
                perform_measure(&ui, &state);
            }
        });
    }

    // --- Confirm overwrite: user accepted; go ahead and re-measure. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_confirm_overwrite(move || {
            let ui = ui.unwrap();
            ui.set_confirm_overwrite_open(false);
            perform_measure(&ui, &state);
        });
    }

    // --- Next: select the following sample, or create a new one at the end. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_next_sample(move || {
            let ui = ui.unwrap();
            let status = {
                let mut st = state.borrow_mut();
                st.blank_preview = None;
                st.advance_selection()
            };
            ui.set_status_text(status.into());
            refresh(&ui, &state);
        });
    }

    // --- Select a table row: plain click = only this row; Shift = add/remove. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_select_row(move |i, additive| {
            let ui = ui.unwrap();
            {
                let mut st = state.borrow_mut();
                st.blank_preview = None;
                let i = i as usize;
                if additive {
                    // Toggle just this row, keeping the rest of the selection.
                    if let Some(s) = st.samples.get_mut(i) {
                        s.selected = !s.selected;
                    }
                    // Keep `current` on a still-selected row (prefer the toggled one).
                    let toggled = st.samples.get(i).filter(|s| s.selected).map(|s| s.number);
                    st.current = toggled
                        .or_else(|| st.samples.iter().find(|s| s.selected).map(|s| s.number));
                } else {
                    // Select only this row.
                    let mut current = None;
                    for (j, s) in st.samples.iter_mut().enumerate() {
                        s.selected = j == i;
                        if j == i {
                            current = Some(s.number);
                        }
                    }
                    st.current = current;
                }
            }
            refresh(&ui, &state);
        });
    }

    // --- Rename the currently selected sample(s). ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_rename_selected(move |name| {
            let ui = ui.unwrap();
            let name = name.trim();
            if name.is_empty() {
                ui.set_status_text("Enter a name to rename the selected sample.".into());
                return;
            }
            let mut renamed = 0;
            {
                let mut st = state.borrow_mut();
                for s in st.samples.iter_mut().filter(|s| s.selected) {
                    s.id = name.to_string();
                    renamed += 1;
                }
            }
            if renamed == 0 {
                ui.set_status_text("Select a sample to rename.".into());
                return;
            }
            ui.set_rename_text("".into());
            ui.set_status_text(format!("Renamed {renamed} sample(s) to “{name}”.").into());
            refresh(&ui, &state);
        });
    }

    // --- Sample type changed: only affects the next measurement. ---
    {
        let ui = ui.as_weak();
        ui.unwrap().on_sample_type_changed(move |idx| {
            let ui = ui.unwrap();
            ui.set_status_text(
                format!(
                    "Sample type set to {} for the next measurement.",
                    type_name(idx)
                )
                .into(),
            );
        });
    }

    // --- Edit > Remove Selected. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_remove_selected(move || {
            let ui = ui.unwrap();
            {
                let mut st = state.borrow_mut();
                st.blank_preview = None;
                let before = st.samples.len();
                st.samples.retain(|s| !s.selected);
                let removed = before - st.samples.len();
                // Drop the current pointer if its sample was removed.
                if let Some(n) = st.current {
                    if !st.samples.iter().any(|s| s.number == n) {
                        st.current = None;
                    }
                }
                ui.set_status_text(format!("Removed {removed} sample(s).").into());
            }
            refresh(&ui, &state);
        });
    }

    // --- Edit > Clear All. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_clear_all(move || {
            let ui = ui.unwrap();
            {
                let mut st = state.borrow_mut();
                st.samples.clear();
                st.next_number = 1;
                st.current = None;
                st.blank_preview = None;
            }
            ui.set_status_text("Cleared all samples.".into());
            refresh(&ui, &state);
        });
    }

    // --- File > Export PDF. ---
    {
        let ui = ui.as_weak();
        let state = state.clone();
        ui.unwrap().on_export_pdf(move || {
            let ui = ui.unwrap();
            export_pdf(&ui, &state);
        });
    }

    // --- File > Quit. ---
    {
        ui.on_request_quit(move || {
            let _ = slint::quit_event_loop();
        });
    }

    ui.run()?;
    Ok(())
}

/// Seed a few representative demo samples so the app opens populated (and to
/// keep `docs/images/opendrop.png` in sync). A throwaway zero-delay mock
/// synthesizes the spectra so startup stays instant; the first two are selected
/// so they overlay on the plot.
fn seed_demo(state: &Rc<RefCell<AppState>>) {
    // (name, sample-type index): dsDNA / dsDNA / dsDNA / RNA.
    let demos: [(&str, i32); 4] = [
        ("λ DNA ladder", 0),
        ("Plasmid miniprep", 0),
        ("gDNA extract", 0),
        ("Total RNA", 1),
    ];

    let mut dev = MockSpectrometer::new();
    dev.set_delay(Duration::ZERO);
    if dev.blank().is_err() {
        return;
    }

    let mut st = state.borrow_mut();
    for (i, (name, type_index)) in demos.iter().enumerate() {
        let Ok(spectrum) = dev.measure() else {
            continue;
        };
        let result = nucleic_acid(&spectrum, sample_type_for(*type_index, 30.0));
        let number = st.next_number;
        st.next_number += 1;
        st.samples.push(Sample {
            number,
            id: (*name).to_string(),
            filled: Some(Filled {
                type_index: *type_index,
                spectrum,
                result,
            }),
            selected: i < 2, // overlay the first two by default
        });
    }
    st.current = st.samples.first().map(|s| s.number);
}

/// Record a blank (simulated ~1 s), verify it took, and reflect the outcome:
/// a green check on the pressed button plus a transient blank trace on the plot
/// on success, or a red cross plus an alert dialog on failure. Each button's
/// glyph is independent — a Re-blank leaves the Blank check as it was. The busy
/// overlay is shown for the duration.
fn perform_blank(ui: &AppWindow, state: &Rc<RefCell<AppState>>, reblank: bool) {
    ui.set_busy_text("Recording blank…".into());
    ui.set_busy_open(true);

    let ui_weak = ui.as_weak();
    let state = state.clone();
    // Defer past the current turn so the busy overlay paints before the
    // blocking (simulated) acquisition runs on the event-loop thread.
    Timer::single_shot(Duration::from_millis(80), move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let outcome = {
            let mut st = state.borrow_mut();
            match st.device.blank() {
                Ok(()) => st.device.verify_blank(),
                Err(e) => Err(e),
            }
        };
        ui.set_busy_open(false);
        match outcome {
            Ok(spectrum) => {
                ui.set_has_blank(true);
                // Green check on the pressed button only; leave the other as-is.
                if reblank {
                    ui.set_reblank_status(1);
                } else {
                    ui.set_blank_status(1);
                }
                let created = {
                    let mut st = state.borrow_mut();
                    // Bootstrap a first sample on Re-blank (only) when none exists
                    // yet, so Measure becomes usable. A plain Blank never adds one.
                    let created = if reblank && st.samples.is_empty() {
                        Some(st.advance_selection())
                    } else {
                        None
                    };
                    // Show the blank re-read on the plot until the next action.
                    st.blank_preview = Some(spectrum);
                    created
                };
                let base = if reblank {
                    "New blank recorded — showing blank trace."
                } else {
                    "Blank recorded — showing blank trace."
                };
                match created {
                    Some(msg) => ui.set_status_text(format!("{base} {msg}").into()),
                    None => ui.set_status_text(base.into()),
                }
                refresh(&ui, &state);
            }
            Err(e) => {
                ui.set_has_blank(false);
                // Red cross on the pressed button only; leave the other as-is.
                if reblank {
                    ui.set_reblank_status(2);
                } else {
                    ui.set_blank_status(2);
                }
                state.borrow_mut().blank_preview = None;
                ui.set_blank_fail_text(format!("{e}").into());
                ui.set_blank_fail_open(true);
                ui.set_status_text("Blank check failed — see dialog.".into());
                refresh(&ui, &state);
            }
        }
    });
}

/// Show the busy overlay, run the (simulated ~1 s) measurement after the first
/// paint, then write the reading into the single selected sample and close the
/// overlay. Caller guarantees exactly one sample is selected.
fn perform_measure(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let type_index = ui.get_sample_type_index();
    let other: f64 = ui.get_custom_constant().parse().unwrap_or(30.0);
    ui.set_busy_text("Measuring…".into());
    ui.set_busy_open(true);

    let ui_weak = ui.as_weak();
    let state = state.clone();
    Timer::single_shot(Duration::from_millis(80), move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let measured = state.borrow_mut().device.measure();
        ui.set_busy_open(false);
        match measured {
            Ok(spectrum) => {
                let result = nucleic_acid(&spectrum, sample_type_for(type_index, other));
                let filled = Filled {
                    type_index,
                    spectrum,
                    result,
                };
                let number = {
                    let mut st = state.borrow_mut();
                    // The measured sample takes over the plot from the blank preview.
                    st.blank_preview = None;
                    match st.single_selected_index() {
                        Some(i) => {
                            st.samples[i].filled = Some(filled);
                            Some(st.samples[i].number)
                        }
                        None => None,
                    }
                };
                match number {
                    Some(n) => ui.set_status_text(format!("Measured sample #{n}.").into()),
                    None => ui.set_status_text("Select a single sample to measure.".into()),
                }
                refresh(&ui, &state);
            }
            Err(DeviceError::NoBlank) => ui.set_status_text("Record a blank first.".into()),
            Err(e) => ui.set_status_text(format!("Measure failed: {e}").into()),
        }
    });
}

/// Map the sample-type dropdown index to a core `SampleType`. `other` is the
/// user-entered ng/µL-per-AU constant used for the "Other" type.
fn sample_type_for(index: i32, other: f64) -> SampleType {
    match index {
        0 => SampleType::DsDna,
        1 => SampleType::Rna,
        2 => SampleType::SsDna,
        _ => SampleType::Custom(other),
    }
}

/// Short label for a sample-type index (used in the table + status line).
fn type_name(index: i32) -> &'static str {
    match index {
        0 => "dsDNA",
        1 => "RNA",
        2 => "ssDNA",
        _ => "Other",
    }
}

/// Rebuild both the table model and the plot from the current sample list.
fn refresh(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();

    // Gate the selection-dependent controls: Measure needs exactly one row,
    // Rename / Delete Selected need at least one.
    let selected_count = st.samples.iter().filter(|s| s.selected).count();
    ui.set_single_selected(selected_count == 1);
    ui.set_has_selection(selected_count >= 1);

    // ---- Table rows ----
    let dash = "—".to_string();
    let rows: Vec<SampleRow> = st
        .samples
        .iter()
        .map(|s| {
            let (r, g, b) = s.color();
            let (type_name_s, a260, a280, r280, r230, conc) = match &s.filled {
                Some(f) => (
                    type_name(f.type_index).to_string(),
                    format!("{:.3}", f.result.a260),
                    format!("{:.3}", f.result.a280),
                    fmt_ratio(f.result.ratio_260_280),
                    fmt_ratio(f.result.ratio_260_230),
                    format!("{:.1}", f.result.concentration_ng_per_ul),
                ),
                None => (
                    dash.clone(),
                    dash.clone(),
                    dash.clone(),
                    dash.clone(),
                    dash.clone(),
                    dash.clone(),
                ),
            };
            SampleRow {
                number: s.number,
                id: s.id.clone().into(),
                type_name: type_name_s.into(),
                a260: a260.into(),
                a280: a280.into(),
                r260_280: r280.into(),
                r260_230: r230.into(),
                conc: conc.into(),
                color: Color::from_rgb_u8(r, g, b),
                selected: s.selected,
            }
        })
        .collect();
    ui.set_samples(ModelRc::new(VecModel::from(rows)));

    // ---- Plot traces ----
    // A fresh blank re-read takes over the plot until the next action; otherwise
    // overlay the selected filled samples.
    let (traces, y_max, y_min) = match st.blank_preview.as_ref() {
        Some(spectrum) => build_preview_trace(spectrum),
        None => {
            let displayed = displayed_indices(&st.samples);
            build_traces(&st.samples, &displayed)
        }
    };
    ui.set_traces(ModelRc::new(VecModel::from(traces)));
    ui.set_y_max_label(format!("{y_max:.1}").into());
    ui.set_y_min_label(fmt_axis_min(y_min).into());
}

/// Format the bottom Y-axis label, collapsing a near-zero (possibly slightly
/// negative) minimum to a clean "0.0" instead of "-0.0".
fn fmt_axis_min(bottom: f64) -> String {
    if bottom.abs() < 0.05 {
        "0.0".to_string()
    } else {
        format!("{bottom:.1}")
    }
}

/// Neutral grey for the transient blank preview trace (Theme.text-faint).
const BLANK_PREVIEW_COLOR: (u8, u8, u8) = (0x9a, 0xa4, 0xb2);

/// Build the single grey trace shown right after a Blank/Re-blank. A good blank
/// is flat and near zero, so the Y axis is floored at 1.0 AU — the reference
/// reads as a flat baseline rather than blown-up noise. Returns the traces plus
/// the Y-axis top and bottom used.
fn build_preview_trace(spectrum: &Spectrum) -> (Vec<PlotTrace>, f64, f64) {
    let mut max_a = f64::MIN;
    let mut min_a = f64::MAX;
    for (wl, a) in spectrum.points() {
        if (PLOT_MIN_NM..=PLOT_MAX_NM).contains(&wl) {
            max_a = max_a.max(a);
            min_a = min_a.min(a);
        }
    }
    let top = if max_a.is_finite() {
        (max_a * 1.1).max(1.0)
    } else {
        1.0
    };
    let bottom = min_a.min(0.0);
    let span = (top - bottom).max(1e-6);
    let (r, g, b) = BLANK_PREVIEW_COLOR;
    let trace = PlotTrace {
        commands: trace_commands(spectrum, bottom, span).into(),
        color: Color::from_rgb_u8(r, g, b),
        label: "Blank (preview)".into(),
    };
    (vec![trace], top, bottom)
}

/// Indices of the samples shown on the plot: the selected *filled* ones. Empty
/// or unselected samples are never plotted.
fn displayed_indices(samples: &[Sample]) -> Vec<usize> {
    samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.selected && s.filled.is_some())
        .map(|(i, _)| i)
        .collect()
}

/// Build the overlaid plot traces (SVG paths in a 0..1000 viewbox) for the
/// displayed samples, sharing a common auto-scaled Y axis. Every index in
/// `displayed` is guaranteed to be a filled sample. Returns the traces plus the
/// Y-axis top and bottom used.
fn build_traces(samples: &[Sample], displayed: &[usize]) -> (Vec<PlotTrace>, f64, f64) {
    if displayed.is_empty() {
        return (Vec::new(), 1.0, 0.0);
    }

    // Shared Y scaling across every displayed spectrum in the plotted window.
    let mut max_a = f64::MIN;
    let mut min_a = f64::MAX;
    for &i in displayed {
        for (wl, a) in filled_spectrum(&samples[i]).points() {
            if (PLOT_MIN_NM..=PLOT_MAX_NM).contains(&wl) {
                max_a = max_a.max(a);
                min_a = min_a.min(a);
            }
        }
    }
    let top = if !max_a.is_finite() || max_a <= 0.0 {
        1.0
    } else {
        max_a * 1.1
    };
    let bottom = min_a.min(0.0);
    let span = (top - bottom).max(1e-6);

    let traces = displayed
        .iter()
        .map(|&i| {
            let s = &samples[i];
            let (r, g, b) = s.color();
            PlotTrace {
                commands: trace_commands(filled_spectrum(s), bottom, span).into(),
                color: Color::from_rgb_u8(r, g, b),
                label: format!("#{}  {}", s.number, s.id).into(),
            }
        })
        .collect();
    (traces, top, bottom)
}

/// The spectrum of a sample known to be filled (panics otherwise — callers only
/// pass filled samples).
fn filled_spectrum(sample: &Sample) -> &Spectrum {
    &sample.filled.as_ref().expect("filled sample").spectrum
}

/// SVG path over the 220–350 nm window for one spectrum, given the shared
/// baseline and span used for Y scaling.
fn trace_commands(spectrum: &Spectrum, bottom: f64, span: f64) -> String {
    let mut cmds = String::new();
    let mut started = false;
    for (wl, a) in spectrum.points() {
        if !(PLOT_MIN_NM..=PLOT_MAX_NM).contains(&wl) {
            continue;
        }
        let x = (wl - PLOT_MIN_NM) / (PLOT_MAX_NM - PLOT_MIN_NM) * VIEW_W;
        let y = ((1.0 - (a - bottom) / span) * VIEW_H).clamp(0.0, VIEW_H);
        cmds.push_str(if started { "L " } else { "M " });
        cmds.push_str(&format!("{x:.1} {y:.1} "));
        started = true;
    }
    cmds
}

/// Format a purity ratio; `nucleic_acid` returns `0.0` for an undefined ratio.
fn fmt_ratio(v: f64) -> String {
    if v.is_finite() && v != 0.0 {
        format!("{v:.2}")
    } else {
        "—".to_string()
    }
}

/// File > Export PDF: prompt for a path, then render the displayed plot + full
/// sample table to a PDF.
fn export_pdf(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let path = match rfd::FileDialog::new()
        .set_title("Export PDF")
        .set_file_name("opendrop-report.pdf")
        .add_filter("PDF", &["pdf"])
        .save_file()
    {
        Some(p) => p,
        None => return, // cancelled
    };

    let st = state.borrow();
    let displayed = displayed_indices(&st.samples);

    let traces: Vec<pdf::PdfTrace> = displayed
        .iter()
        .map(|&i| {
            let s = &st.samples[i];
            pdf::PdfTrace {
                color: s.color(),
                label: format!("#{}  {}", s.number, s.id),
                points: filled_spectrum(s)
                    .points()
                    .filter(|(wl, _)| (PLOT_MIN_NM..=PLOT_MAX_NM).contains(wl))
                    .collect(),
            }
        })
        .collect();

    let rows: Vec<Vec<String>> = st
        .samples
        .iter()
        .map(|s| {
            let (ty, a260, a280, r280, r230, conc) = match &s.filled {
                Some(f) => (
                    type_name(f.type_index).to_string(),
                    format!("{:.3}", f.result.a260),
                    format!("{:.3}", f.result.a280),
                    fmt_ratio(f.result.ratio_260_280),
                    fmt_ratio(f.result.ratio_260_230),
                    format!("{:.1}", f.result.concentration_ng_per_ul),
                ),
                None => (
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                ),
            };
            vec![
                s.number.to_string(),
                s.id.clone(),
                ty,
                a260,
                a280,
                r280,
                r230,
                conc,
            ]
        })
        .collect();

    let report = pdf::Report {
        device_text: ui.get_device_text().to_string(),
        traces,
        x_range: (PLOT_MIN_NM, PLOT_MAX_NM),
        rows,
    };
    drop(st);

    match pdf::export(&path, &report) {
        Ok(()) => ui.set_status_text(format!("Exported PDF to {}", path.display()).into()),
        Err(e) => ui.set_status_text(format!("PDF export failed: {e}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(number: i32, selected: bool, filled: bool) -> Sample {
        Sample {
            number,
            id: format!("Sample {number}"),
            filled: filled.then(|| Filled {
                type_index: 0,
                spectrum: Spectrum::zeros(),
                result: nucleic_acid(&Spectrum::zeros(), SampleType::DsDna),
            }),
            selected,
        }
    }

    #[test]
    fn displayed_indices_include_only_selected_filled_samples() {
        let samples = vec![
            sample(1, true, true),
            sample(2, false, true),
            sample(3, true, false),
        ];

        assert_eq!(displayed_indices(&samples), vec![0]);
    }

    #[test]
    fn displayed_indices_are_empty_when_all_rows_are_deselected() {
        let samples = vec![sample(1, false, true), sample(2, false, true)];

        assert!(displayed_indices(&samples).is_empty());
    }
}
