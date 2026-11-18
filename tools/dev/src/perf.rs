//! Budgets on the X4: time per frame, heap per screen, and storage read
//! while drawing (frames are drawn in bands; a screen that reads as it
//! draws reads again in every band), with the largest costs listed.
//!
//! Every screen and widget of the target is opened (a widget placed alone
//! on the home screen's first page) and closed again a few times on the
//! X4 profile (the X4 Pro and X4 Classic too where `--profile` asks for
//! them; the other profiles draw whole frames and have no budget here):
//!
//! - `perf.draw-io`, error: drawing asked the storage where it must not,
//!   by the rule of the guard of the standard app,
//!   `crates/paperpaw-app/tests/draw.rs` (see [`draw_io`]).
//! - `perf.heap`: the most the heap held above what was live before it
//!   opened, while it opened and was drawn. An error where that is more
//!   than the profile's heap leaves free beside what the app itself keeps
//!   on its home screen (the device would run out), a warning above an
//!   eighth of the profile's heap ([`HEAP_SHARE`]: 32 KiB on the X4).
//! - `perf.leak`, warning: more stays on the heap each time it is opened
//!   and closed again.
//! - `perf.frame-time`, warning: a frame estimated to take more than
//!   [`FRAME_BUDGET_MS`] on an X4. Frames are timed on this computer and
//!   scaled by a factor measured in the same run (see [`calibrate`]).
//! - `perf.open-io`, warning: many storage calls before the first frame
//!   (on the X4's card a file looked up in a large folder takes up to
//!   30 ms).
//! - `perf.top`, `perf.heap-sites`, `perf.calibration`, notes: the worst
//!   few by time, heap and storage calls; where a screen's heap goes (the
//!   allocator's call sites, recorded only for screens with heap findings,
//!   or the ten largest with `--verbose`, as recording is slow); the
//!   factor used.
//!
//! Everything measured is listed on `<out>/perf/index.html`.

use std::fmt::Write as _;

use paperpaw_core::sdk::Params;
use paperpaw_core::sdk::builtin::{actions, params, slots};
use paperpaw_emulator::budget;
use paperpaw_emulator::profiles::Profile;
use paperpaw_emulator::trace::FrameRecord;

use crate::cases;
use crate::cli::Context;
use crate::report::{self, Finding, Report, escape};
use crate::session::{self, Session, Start};
use crate::variant::{self, Variant};

/// A frame should take at most this long on an X4: half a second. The
/// home screen takes 330 ms there and the settings 160 ms; with the
/// estimate within about a factor of two, a frame clearly slower than the
/// home screen is what this finds.
pub const FRAME_BUDGET_MS: f64 = 500.0;

/// A screen's peak above this share of the profile's heap is a warning:
/// an eighth, 32 KiB on the X4, so a few such screens and a book still
/// fit together.
pub const HEAP_SHARE: usize = 8;

/// Storage calls (reads, sizes, lists) before a screen's first frame
/// above which it is slow to come up on the X4.
pub const LOOKUPS_BEFORE: u32 = 40;

/// Times a screen is opened and closed; the first fills caches, the
/// others show what stays.
const CYCLES: usize = 4;

/// Times the whole screen is drawn again, the fastest taken.
const AGAIN: usize = 5;

/// Bytes more after every close that make a leak.
const LEAK_BYTES: i64 = 64;

/// Frames measured on an X4: the home screen and
/// the settings, drawn with the firmware as it is now.
const X4_HOME_MS: f64 = 330.0;
const X4_SETTINGS_MS: f64 = 160.0;

/// Cases listed in each ranking on the page, and as notes.
const TOP_PAGE: usize = 15;
const TOP_NOTES: usize = 3;

/// What is opened: a screen (with parameters) or a widget in a size.
#[derive(Debug, Clone)]
enum Case {
    Screen {
        id: String,
        params: Vec<(String, String)>,
    },
    Widget {
        id: String,
        size: (u8, u8),
    },
}

impl Case {
    fn id(&self) -> &str {
        match self {
            Case::Screen { id, .. } | Case::Widget { id, .. } => id,
        }
    }

    fn show(&self, session: &mut Session) -> Result<(), String> {
        match self {
            Case::Screen { id, params } => {
                let params: Vec<(&str, &str)> = params
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                session.open(id, &params)
            }
            Case::Widget { id, size } => {
                session.set(cases::PAGE, &format!("{id} 0/0 {}x{}", size.0, size.1))
            }
        }
    }

    fn hide(&self, session: &mut Session) -> Result<(), String> {
        match self {
            Case::Screen { .. } => {
                cases::home(session);
                Ok(())
            }
            Case::Widget { .. } => session.set(cases::PAGE, ""),
        }
    }

    /// Whether it is shown (a screen may close at once or open another).
    fn shown(&self, session: &Session) -> bool {
        match self {
            Case::Screen { id, .. } => session.screen().as_deref() == Some(id.as_str()),
            Case::Widget { .. } => true,
        }
    }
}

/// What was measured of one screen, or a widget in one size.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    /// The screen's or widget's id.
    pub id: String,
    /// The widget's size, for widgets.
    pub size: Option<(u8, u8)>,
    /// Parameters it was opened with.
    pub params: String,
    /// Milliseconds of the frame it came up with, each time it opened.
    pub came_up: Vec<f64>,
    /// Milliseconds of the whole screen drawn again.
    pub again: Vec<f64>,
    /// Bands of the frame it came up with.
    pub bands: usize,
    /// Reads, sizes and lists before its first frame (the first opening).
    pub before: u32,
    /// Bytes read before its first frame.
    pub before_bytes: u64,
    /// Storage calls drawing the frame it came up with and drawing it
    /// again.
    pub drawing: u32,
    /// What drawing asked the storage that it must not (see [`draw_io`]).
    pub faults: Vec<String>,
    /// The most the heap held above what was live before it opened.
    pub peak: usize,
    /// What stayed live while it was shown, above that.
    pub kept: i64,
    /// What stayed live after each close, above that.
    pub after: Vec<i64>,
    /// Where the heap went while it was shown: site, bytes, allocations.
    pub sites: Vec<(String, usize, usize)>,
    /// The home screen drawn again just before it opened, in ms (with
    /// the first page empty for widgets).
    pub reference: f64,
    /// Its slowest frame estimated on an X4, in ms.
    pub x4_ms: f64,
}

impl Measured {
    /// `notes.list` or `widgets.clock 2x2`.
    pub fn place(&self) -> String {
        match self.size {
            Some((w, h)) => format!("{} {w}x{h}", self.id),
            None => self.id.clone(),
        }
    }

    /// The fastest the whole screen was drawn, in ms here.
    pub fn frame_ms(&self) -> f64 {
        self.again.iter().copied().fold(f64::NAN, f64::min)
    }

    /// The frame it came up with, the fastest of its openings (others
    /// may be slowed by the computer's other work), in ms here.
    pub fn came_up_ms(&self) -> f64 {
        self.came_up.iter().copied().fold(f64::NAN, f64::min)
    }

    /// How much faster the computer is now than when it was calibrated,
    /// from the home screen drawn again before this case (`calibrated`
    /// then): what its frames are multiplied by, within a half and twice.
    pub fn scale(&self, calibrated: f64) -> f64 {
        if self.reference.is_finite() && self.reference > 0.0 {
            (calibrated / self.reference).clamp(0.5, 2.0)
        } else {
            1.0
        }
    }

    /// The slower of the two, in ms here.
    pub fn worst_ms(&self) -> f64 {
        let (a, b) = (self.frame_ms(), self.came_up_ms());
        if a.is_nan() {
            b
        } else if b.is_nan() {
            a
        } else {
            a.max(b)
        }
    }

    /// Bytes more after each close, if it keeps growing (from the second
    /// close on, the first having filled caches).
    pub fn leak(&self) -> Option<i64> {
        let after = self.after.get(1..)?;
        if after.len() < 2 {
            return None;
        }
        let growing = after.windows(2).all(|w| w[1] > w[0]);
        let grown = after[after.len() - 1] - after[0];
        (growing && grown >= LEAK_BYTES * (after.len() as i64 - 1))
            .then(|| grown / (after.len() as i64 - 1))
    }
}

/// The rule of the guard of the standard app,
/// `crates/paperpaw-app/tests/draw.rs`, on one frame: what drawing it did
/// on the storage from band `from` on (0 the first), each once, as
/// `when, band N: op path`. The rows of decoded pictures (`picture` tells
/// their size, `None` for other files) are read while drawing by design,
/// a band at a time; for those only more than each row once a frame is a
/// fault. A frame a screen comes up with may load in its first band
/// (`from` 1); a frame drawn again, nothing changed, may not (`from` 0).
///
/// The guard counts below a kernel of its own; this counts below the
/// emulator's (see `paperpaw_emulator::trace`). A change to the rule
/// belongs in both.
pub fn draw_io(
    frame: &FrameRecord,
    from: usize,
    when: &str,
    picture: &dyn Fn(&str) -> Option<u64>,
) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut pictures: Vec<(&str, u64, u64)> = Vec::new();
    for call in frame.calls.iter().filter(|c| c.band >= from) {
        if matches!(call.op, "read" | "size")
            && let Some(size) = picture(&call.path)
        {
            match pictures.iter_mut().find(|(p, ..)| *p == call.path) {
                Some((_, bytes, _)) => *bytes += call.bytes,
                None => pictures.push((&call.path, call.bytes, size)),
            }
            continue;
        }
        let fault = format!("{when}, band {}: {} {}", call.band + 1, call.op, call.path);
        if !found.contains(&fault) {
            found.push(fault);
        }
    }
    // Each row once a frame; in each band it lies in its header, and rows
    // read with those of the band before (8 KiB at a time).
    let bands = frame.bands.len() as u64;
    for (path, bytes, size) in pictures {
        if bytes > size + bands * (8192 + 64) {
            found.push(format!(
                "{when}: {bytes} bytes of the picture {path} of {size}"
            ));
        }
    }
    found
}

/// The size of `path` in `library` if it is a decoded picture (`PPGI`
/// gray, `PPCI` colour, see `paperpaw_image`).
fn picture_size(library: &dyn paperpaw_platform::Storage, path: &str) -> Option<u64> {
    let path = path.trim_start_matches('/');
    let mut magic = [0u8; 4];
    let read = library.read_at(path, 0, &mut magic).ok()?;
    (read == 4 && (&magic == b"PPGI" || &magic == b"PPCI")).then(|| library.size(path).unwrap_or(0))
}

/// The factor from frames here to frames on an X4.
#[derive(Debug, Clone, Copy)]
pub struct Calibration {
    pub factor: f64,
    /// The home screen and the settings drawn here, in ms.
    pub home_ms: f64,
    pub settings_ms: Option<f64>,
    /// The home screen with its first page empty, where widgets are
    /// placed, in ms.
    pub blank_ms: f64,
}

/// The fastest of a few frames of the screen shown drawn again, in ms.
fn fastest(session: &mut Session, times: usize) -> f64 {
    drop(session.frames());
    let mut best = f64::NAN;
    for _ in 0..times {
        session.device.draw_again();
        for frame in session.frames() {
            best = best.min(frame.millis);
        }
    }
    best.max(0.001)
}

/// Measures the factor from frames here to frames on an X4: the home
/// screen (a book read a little, as on a device in use) and the settings
/// drawn again here, the fastest of a few times, against their times
/// measured on an X4 (330 ms and 160 ms), the geometric mean
/// of the two ratios. Measured in the same run, it follows this computer
/// and how the tool was built; the home screen is drawn again before each
/// case too, and a case's frames are scaled by how much faster or slower
/// it drew then (see [`Measured::scale`]), as other work on the computer
/// comes and goes. It is rough: the X4's numbers are frames as the
/// firmware times them (with the card's reads of the first band and the
/// bands sent to the panel), its home screen had other books, and the C3
/// has no FPU, so screens doing much floating point take longer there;
/// expect estimates to be within about a factor of two.
pub fn calibrate(session: &mut Session) -> Calibration {
    cases::home(session);
    let home_ms = fastest(session, 10);
    let home = session.screen();
    let settings_ms = session
        .run(actions::SETTINGS.as_str())
        .ok()
        .filter(|()| session.screen().is_some() && session.screen() != home)
        .map(|()| fastest(session, 10));
    cases::home(session);
    let page = session
        .device
        .kernel
        .settings()
        .text(cases::PAGE)
        .to_string();
    let blank_ms = match session.set(cases::PAGE, "") {
        Ok(()) => fastest(session, 10),
        Err(_) => home_ms,
    };
    let _ = session.set(cases::PAGE, &page);
    Calibration::of(home_ms, settings_ms, blank_ms)
}

impl Calibration {
    fn of(home_ms: f64, settings_ms: Option<f64>, blank_ms: f64) -> Self {
        let factor = match settings_ms {
            Some(s) => ((X4_HOME_MS / home_ms) * (X4_SETTINGS_MS / s)).sqrt(),
            None => X4_HOME_MS / home_ms,
        };
        Calibration {
            factor,
            home_ms,
            settings_ms,
            blank_ms,
        }
    }

    /// Measured before and after the cases, the computer's speed may
    /// change on the way (its clock, other work): the mean of both.
    fn and(self, after: Calibration) -> Self {
        let settings_ms = match (self.settings_ms, after.settings_ms) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            (a, b) => a.or(b),
        };
        Self::of(
            (self.home_ms + after.home_ms) / 2.0,
            settings_ms,
            (self.blank_ms + after.blank_ms) / 2.0,
        )
    }

    /// What a case's frames are multiplied by to be the X4's.
    fn x4(&self, m: &Measured) -> f64 {
        let reference = if m.size.is_some() {
            self.blank_ms
        } else {
            self.home_ms
        };
        self.factor * m.scale(reference)
    }
}

/// What one profile's run measured.
#[derive(Debug)]
struct Run {
    variant: Variant,
    calibration: Calibration,
    /// The heap the profile has, and what the app keeps on its home
    /// screen.
    limit: Option<usize>,
    app: usize,
    measured: Vec<Measured>,
    /// Screens that showed something else when opened.
    skipped: Vec<String>,
}

impl Run {
    fn free(&self) -> Option<usize> {
        self.limit.map(|l| l.saturating_sub(self.app))
    }

    fn share(&self) -> Option<usize> {
        self.limit.map(|l| l / HEAP_SHARE)
    }
}

/// The profiles budgets are measured on: those asked for that draw in
/// bands with a heap budget (the Xteink ones); only the X4 unless
/// `--profile` named others.
fn profiles(context: &Context) -> Vec<&'static Profile> {
    let defaults = context
        .profiles
        .iter()
        .map(|p| p.device.id)
        .eq(variant::DEFAULT_PROFILES.iter().copied());
    context
        .profiles
        .iter()
        .copied()
        .filter(|p| p.frame_rows.is_some() && p.heap_budget.is_some())
        .filter(|p| !defaults || p.device.id == "xteink-x4")
        .collect()
}

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let profiles = profiles(context);
    if profiles.is_empty() {
        report.add(Finding::info(
            "perf.skipped",
            "profiles",
            "Budgets are measured on the Xteink profiles only (they draw in bands and have a \
             heap budget); none was asked for.",
        ));
        return Ok(());
    }
    let language = context
        .languages
        .as_ref()
        .and_then(|l| l.first().cloned())
        .unwrap_or_else(|| String::from("en"));
    let mut runs = Vec::new();
    for profile in profiles {
        let variant = Variant {
            language: language.clone(),
            ..Variant::of(profile)
        };
        eprintln!("paperpaw-dev: perf on {}", variant.name());
        let run = measure_variant(context, &variant)?;
        findings(&run, report);
        runs.push(run);
    }
    if context.in_memory {
        return Ok(());
    }
    let dir = context.path("perf");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join("index.html");
    std::fs::write(&file, html(&runs)).map_err(|e| format!("{}: {e}", file.display()))?;
    report.page("Budgets on the X4", "perf/index.html");
    Ok(())
}

/// Opens and closes every screen and widget of the target on one profile.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn measure_variant(context: &Context, variant: &Variant) -> Result<Run, String> {
    // The command line's settings come with every session.
    let start = Start::default();
    // Heap of no session's: another device running beside the checks
    // (the browser's emulator), not what the app keeps.
    let outside = budget::stats().live;
    let mut session = Session::start(context, variant, &start)?;
    // A book read a little, as on a device in use: the home screen and
    // the book widgets show it, with its cover.
    session.device.call(|kernel, now| {
        let book = Params::new().with(params::PATH, session::BOOK);
        kernel.open_slot(now, slots::READER, book)
    });
    // Not settled: the reader counts the book's pages in the background,
    // which takes long and is not what is measured here.
    for _ in 0..2 {
        session.device.run_action(actions::NEXT_PAGE);
    }
    cases::home(&mut session);
    let calibration = calibrate(&mut session);
    drop(session.frames());
    let app = budget::stats().live.saturating_sub(outside);
    let mut run = Run {
        variant: variant.clone(),
        calibration,
        limit: variant.profile.heap_budget,
        app,
        measured: Vec::new(),
        skipped: Vec::new(),
    };
    let mut cases_done: Vec<Case> = Vec::new();
    for id in cases::screens(&session, &context.target) {
        let mut tried = None;
        for params in [
            Vec::new(),
            vec![(params::PATH.to_string(), session::BOOK.to_string())],
        ] {
            let case = Case::Screen {
                id: id.clone(),
                params,
            };
            if let Some(measured) = measure(&mut session, &case, CYCLES)? {
                tried = Some((case, measured));
                break;
            }
        }
        match tried {
            Some((case, measured)) => {
                cases_done.push(case);
                run.measured.push(measured);
            }
            None => run.skipped.push(id),
        }
    }
    let widgets = cases::widgets(&session, &context.target);
    if !widgets.is_empty() {
        let grid = cases::grid(&session);
        let sizes = if context.quick {
            let size = (2.min(grid.0), 2.min(grid.1));
            vec![size]
        } else {
            cases::some_sizes(grid)
        };
        let page = session
            .device
            .kernel
            .settings()
            .text(cases::PAGE)
            .to_string();
        for (id, _) in &widgets {
            for (n, &size) in sizes.iter().enumerate() {
                let case = Case::Widget {
                    id: id.clone(),
                    size,
                };
                // Open and closed several times in one size, enough for
                // leaks; twice in the others, for a time to trust.
                let cycles = if n == 0 { CYCLES } else { 2 };
                if let Some(measured) = measure(&mut session, &case, cycles)? {
                    cases_done.push(case);
                    run.measured.push(measured);
                }
            }
        }
        session.set(cases::PAGE, &page)?;
    }
    // Before allocations are recorded, which makes everything slower.
    run.calibration = run.calibration.and(calibrate(&mut session));
    for m in &mut run.measured {
        let ms = m.worst_ms();
        // No frame drawn: nothing to estimate.
        m.x4_ms = if ms.is_nan() {
            0.0
        } else {
            ms * run.calibration.x4(m)
        };
    }
    // Where the heap goes, for screens with heap findings (or the largest
    // with --verbose): slow, so only now.
    let mut asked: Vec<usize> = (0..run.measured.len())
        .filter(|&i| {
            heap_level(&run, &run.measured[i]).is_some() || run.measured[i].leak().is_some()
        })
        .collect();
    if context.verbose {
        let mut largest: Vec<usize> = (0..run.measured.len()).collect();
        largest.sort_by_key(|&i| std::cmp::Reverse(run.measured[i].peak));
        for i in largest.into_iter().take(10) {
            if !asked.contains(&i) {
                asked.push(i);
            }
        }
    }
    if !asked.is_empty() {
        budget::profile();
        for i in asked {
            let case = &cases_done[i];
            case.hide(&mut session)?;
            case.show(&mut session)?;
            run.measured[i].sites = budget::sites(3).into_iter().take(10).collect();
            case.hide(&mut session)?;
        }
    }
    drop(session.frames());
    Ok(run)
}

/// Opens `case` and closes it `cycles` times; `None` if a screen showed
/// something else.
fn measure(session: &mut Session, case: &Case, cycles: usize) -> Result<Option<Measured>, String> {
    let library = session
        .device
        .library()
        .open()
        .map_err(|e| format!("library: {e}"))?;
    let picture = |path: &str| picture_size(&library, path);
    case.hide(session)?;
    let reference = fastest(session, 3);
    drop(session.frames());
    let base = budget::stats().live;
    let mut measured = Measured {
        id: case.id().to_string(),
        size: match case {
            Case::Widget { size, .. } => Some(*size),
            Case::Screen { .. } => None,
        },
        params: match case {
            Case::Screen { params, .. } => params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" "),
            Case::Widget { .. } => String::new(),
        },
        reference,
        ..Measured::default()
    };
    let live = || budget::stats().live as i64 - base as i64;
    for cycle in 0..cycles {
        drop(session.frames());
        budget::reset_peak();
        case.show(session)?;
        let frames = session.frames();
        if !case.shown(session) {
            case.hide(session)?;
            return Ok(None);
        }
        if let Some(last) = frames.last() {
            measured.came_up.push(last.millis);
        }
        if cycle == 0 {
            if let Some(first) = frames.first() {
                let b = &first.before;
                measured.before = b.reads + b.sizes + b.lists;
                measured.before_bytes = b.bytes;
            }
            if let Some(last) = frames.last() {
                measured.bands = last.bands.len();
                measured.drawing += last.drawing.calls();
                measured.faults = draw_io(last, 1, "came up", &picture);
            }
            for n in 0..AGAIN {
                session.device.draw_again();
                let again = session.frames();
                let Some(frame) = again.first() else {
                    continue;
                };
                measured.again.push(frame.millis);
                if n == 0 {
                    measured.drawing += frame.drawing.calls();
                    for fault in draw_io(frame, 0, "drawn again", &picture) {
                        if !measured.faults.contains(&fault) {
                            measured.faults.push(fault);
                        }
                    }
                }
            }
            measured.kept = live();
        }
        let peak = budget::stats().peak.saturating_sub(base);
        measured.peak = measured.peak.max(peak);
        case.hide(session)?;
        // A request the screen started (a list fetched on opening) is not
        // kept: it is let end before the heap is looked at.
        session.drain_requests();
        measured.after.push(live());
    }
    drop(session.frames());
    Ok(Some(measured))
}

fn kib(bytes: impl Into<f64>) -> String {
    format!("{:.1} KiB", bytes.into() / 1024.0)
}

/// The namespace of `id`: the plugin it belongs to.
fn plugin_of(id: &str) -> &str {
    id.split(['.', '@', ' ', '/']).next().unwrap_or(id)
}

/// Whether a measured peak is an error or warning on this run's profile.
fn heap_level(run: &Run, m: &Measured) -> Option<crate::report::Level> {
    use crate::report::Level;
    if run.free().is_some_and(|free| m.peak > free) {
        Some(Level::Error)
    } else if run.share().is_some_and(|share| m.peak > share) {
        Some(Level::Warning)
    } else {
        None
    }
}

/// Findings on one measured case, merged for a widget's sizes.
struct Grouped<'a> {
    id: &'a str,
    widget: bool,
    sizes: Vec<&'a Measured>,
}

fn grouped(run: &Run) -> Vec<Grouped<'_>> {
    let mut groups: Vec<Grouped<'_>> = Vec::new();
    for m in &run.measured {
        match groups
            .iter_mut()
            .find(|g| g.id == m.id && g.widget == m.size.is_some())
        {
            Some(group) => group.sizes.push(m),
            None => groups.push(Grouped {
                id: &m.id,
                widget: m.size.is_some(),
                sizes: vec![m],
            }),
        }
    }
    groups
}

/// `in 1x1, 2x1: ` for a widget's sizes, nothing for a screen.
fn in_sizes(sizes: &[&Measured]) -> String {
    let named: Vec<String> = sizes
        .iter()
        .filter_map(|m| m.size.map(|(w, h)| format!("{w}x{h}")))
        .collect();
    if named.is_empty() {
        String::new()
    } else {
        format!("In {}: ", named.join(", "))
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn findings(run: &Run, report: &mut Report) {
    let variant = run.variant.name();
    let device = run.variant.profile.device.name;
    let factor = run.calibration.factor;
    let add = |report: &mut Report, finding: Finding, id: &str| {
        report.add(finding.plugin(plugin_of(id)).variant(variant.clone()));
    };
    let c = &run.calibration;
    report.add(
        Finding::info(
            "perf.calibration",
            "frames",
            format!(
                "Frame times on the X4 are estimated as {factor:.0} times those here: the home \
                 screen with a book read drew in {:.2} ms here (330 ms on an X4){}, before and \
                 after the rest; rough, within about a factor of two.",
                c.home_ms,
                c.settings_ms
                    .map(|s| format!(", the settings in {s:.2} ms (160 ms)"))
                    .unwrap_or_default()
            ),
        )
        .variant(variant.clone()),
    );
    if let (Some(limit), Some(free)) = (run.limit, run.free()) {
        report.add(
            Finding::info(
                "perf.calibration",
                "heap",
                format!(
                    "Of the {} heap, the app keeps {} on its home screen, leaving {} for a screen; \
                     a screen taking more than {} warns.",
                    kib(limit as f64),
                    kib(run.app as f64),
                    kib(free as f64),
                    kib((limit / HEAP_SHARE) as f64)
                ),
            )
            .variant(variant.clone()),
        );
    }
    if !run.skipped.is_empty() {
        report.add(
            Finding::info(
                "perf.skipped",
                "screens",
                format!(
                    "Not measured, as opening them without parameters (or with a book's path) \
                     showed another screen: {}.",
                    run.skipped.join(", ")
                ),
            )
            .variant(variant.clone()),
        );
    }
    for group in grouped(run) {
        let id = group.id;
        // Storage while drawing.
        let faulty: Vec<&Measured> = group
            .sizes
            .iter()
            .copied()
            .filter(|m| !m.faults.is_empty())
            .collect();
        if let Some(first) = faulty.first() {
            let mut faults = first.faults.clone();
            let more = faults.len().saturating_sub(6);
            faults.truncate(6);
            let more = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            let sizes = if group.widget {
                in_sizes(&faulty)
            } else {
                String::new()
            };
            add(
                report,
                Finding::error(
                    "perf.draw-io",
                    id,
                    format!(
                        "{sizes}Asks the storage while it draws (a frame of {} bands): {}{more}. \
                         Load what `draw` shows when it opens, is placed or its data changes, \
                         and keep it (docs/plugins.md, \"`draw` only draws\").",
                        first.bands,
                        faults.join("; ")
                    ),
                ),
                id,
            );
        }
        // Heap.
        let worst = group.sizes.iter().max_by_key(|m| m.peak);
        if let Some(m) = worst
            && let Some(level) = heap_level(run, m)
        {
            let sizes = if group.widget {
                in_sizes(&[m])
            } else {
                String::new()
            };
            let message = match level {
                crate::report::Level::Error => format!(
                    "{sizes}Takes up to {} of heap opening and drawing, more than the {} the \
                     {device} has free beside the app: it would run out of memory there. Keep less in \
                     memory (lists in files, pieces read as needed) or check \
                     `env.device().memory` to do less on small devices.",
                    kib(m.peak as f64),
                    kib(run.free().unwrap_or(0) as f64)
                ),
                _ => format!(
                    "{sizes}Takes up to {} of heap opening and drawing, more than an eighth of \
                     the profile's heap ({}); with a book open or another screen below it may not \
                     fit. Keep less in memory, or check `env.device().memory` to do less on small \
                     devices.",
                    kib(m.peak as f64),
                    kib(run.share().unwrap_or(0) as f64)
                ),
            };
            add(report, Finding::new("perf.heap", level, id, message), id);
        }
        // Leaks.
        if let Some(m) = group.sizes.iter().find(|m| m.leak().is_some()) {
            let grown = m.leak().unwrap_or(0);
            add(
                report,
                Finding::warning(
                    "perf.leak",
                    id,
                    format!(
                        "Leaves {grown} bytes more on the heap each time it is opened and closed \
                         again (after each close: {}); something kept across openings grows \
                         (a static, a cache without a bound).",
                        m.after
                            .iter()
                            .map(|b| format!("{b:+} B"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ),
                id,
            );
        }
        // Time.
        let slowest = group
            .sizes
            .iter()
            .max_by(|a, b| a.x4_ms.total_cmp(&b.x4_ms));
        if let Some(m) = slowest
            && m.x4_ms > FRAME_BUDGET_MS
        {
            let sizes = if group.widget {
                in_sizes(&[m])
            } else {
                String::new()
            };
            add(
                report,
                Finding::warning(
                    "perf.frame-time",
                    id,
                    format!(
                        "{sizes}A frame takes about {:.0} ms on an X4 (estimated; {:.1} ms here \
                         as it comes up, {:.1} ms drawn again), over the {FRAME_BUDGET_MS:.0} ms \
                         a frame should take there. Lay out and measure when it opens or its \
                         data changes, not in `draw`.",
                        m.x4_ms,
                        m.came_up_ms(),
                        m.frame_ms()
                    ),
                ),
                id,
            );
        }
        // Storage before the first frame.
        if let Some(m) = group.sizes.iter().max_by_key(|m| m.before)
            && m.before > LOOKUPS_BEFORE
        {
            let sizes = if group.widget {
                in_sizes(&[m])
            } else {
                String::new()
            };
            add(
                report,
                Finding::warning(
                    "perf.open-io",
                    id,
                    format!(
                        "{sizes}Asks the storage {} times ({} read) before its first frame; on \
                         the X4's card each lookup can take up to 30 ms. Read less when it \
                         opens, or show it first and load the rest in steps.",
                        m.before,
                        kib(m.before_bytes as f64)
                    ),
                ),
                id,
            );
        }
        // Where the heap went.
        if let Some(m) = group.sizes.iter().find(|m| !m.sites.is_empty()) {
            let sites: Vec<String> = m
                .sites
                .iter()
                .take(3)
                .map(|(site, bytes, count)| {
                    format!("{} in {count} at {}", kib(*bytes as f64), one_line(site))
                })
                .collect();
            add(
                report,
                Finding::info(
                    "perf.heap-sites",
                    m.place(),
                    format!("Most heap while shown: {}.", sites.join("; ")),
                ),
                id,
            );
        }
    }
    // The worst few.
    let measured: Vec<&Measured> = run.measured.iter().collect();
    for (what, list) in rankings(&measured) {
        for m in list.into_iter().take(TOP_NOTES) {
            let message = match what {
                Ranking::Time => format!(
                    "Among the slowest: about {:.0} ms a frame on an X4 (estimated; {:.2} ms here).",
                    m.x4_ms,
                    m.worst_ms()
                ),
                Ranking::Heap => format!(
                    "Among the most heap: up to {} opening and drawing, {} kept while shown.",
                    kib(m.peak as f64),
                    kib(m.kept as f64)
                ),
                Ranking::Storage => format!(
                    "Among the most storage calls: {} before its first frame, {} drawing it.",
                    m.before, m.drawing
                ),
            };
            add(report, Finding::info("perf.top", m.place(), message), &m.id);
        }
    }
}

/// A call site of [`budget::sites`] on one line.
fn one_line(site: &str) -> String {
    site.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Copy)]
enum Ranking {
    Time,
    Heap,
    Storage,
}

/// The cases, slowest, most heap and most storage calls first; a widget
/// once (in its worst size).
fn rankings<'a>(measured: &[&'a Measured]) -> Vec<(Ranking, Vec<&'a Measured>)> {
    let once = |mut list: Vec<&'a Measured>| {
        let mut seen: Vec<&str> = Vec::new();
        list.retain(|m| {
            let first = !seen.contains(&m.id.as_str());
            seen.push(&m.id);
            first
        });
        list
    };
    let mut time = measured.to_vec();
    time.sort_by(|a, b| b.x4_ms.total_cmp(&a.x4_ms));
    let mut heap = measured.to_vec();
    heap.sort_by_key(|m| std::cmp::Reverse(m.peak));
    let mut storage = measured.to_vec();
    storage.sort_by_key(|m| std::cmp::Reverse(m.before + m.drawing));
    storage.retain(|m| m.before + m.drawing > 0);
    vec![
        (Ranking::Time, once(time)),
        (Ranking::Heap, once(heap)),
        (Ranking::Storage, once(storage)),
    ]
}

/// `<out>/perf/index.html`: how it was measured, the rankings, and every
/// case.
fn html(runs: &[Run]) -> String {
    let mut body = String::from("<h1>Budgets on the X4</h1>");
    let _ = write!(
        body,
        "<p>Every screen and widget opened and closed {CYCLES} times (a widget alone on the \
         home screen's first page, in several sizes). Frame times are measured here and \
         scaled to an X4 by a factor measured in the same run (rough, within about a factor \
         of two); heap is the most held above what was live before opening, with the \
         emulator's allocator (about a fifth more than on the device). Budgets: a frame \
         {FRAME_BUDGET_MS:.0} ms on an X4, a screen's heap an eighth of the profile's, at most \
         what the app leaves free; no storage calls drawing after the first band of a new \
         screen, none drawn again (picture rows excepted).</p>"
    );
    for run in runs {
        let factor = run.calibration.factor;
        let _ = write!(
            body,
            "<h2>{}</h2><p>Factor to the X4: {factor:.0} (home screen {:.2} ms here{}). ",
            escape(&run.variant.name()),
            run.calibration.home_ms,
            run.calibration
                .settings_ms
                .map(|s| format!(", settings {s:.2} ms"))
                .unwrap_or_default()
        );
        if let (Some(limit), Some(free)) = (run.limit, run.free()) {
            let _ = write!(
                body,
                "Heap {}, the app on its home screen {}, free {}, warning above {}.",
                kib(limit as f64),
                kib(run.app as f64),
                kib(free as f64),
                kib((limit / HEAP_SHARE) as f64)
            );
        }
        body.push_str("</p>");
        if !run.skipped.is_empty() {
            let _ = write!(
                body,
                "<p>Not measured (showed another screen): {}.</p>",
                escape(&run.skipped.join(", "))
            );
        }
        let measured: Vec<&Measured> = run.measured.iter().collect();
        for (what, list) in rankings(&measured) {
            let title = match what {
                Ranking::Time => "Slowest frames",
                Ranking::Heap => "Most heap",
                Ranking::Storage => "Most storage calls",
            };
            let _ = write!(body, "<h3>{title}</h3>");
            if list.is_empty() {
                body.push_str("<p>None.</p>");
            } else {
                table(&mut body, run, list.into_iter().take(TOP_PAGE));
            }
        }
        body.push_str("<h3>Everything</h3>");
        let mut all = measured.clone();
        all.sort_by_key(|m| m.place());
        table(&mut body, run, all.into_iter());
        let with_sites: Vec<&Measured> = run
            .measured
            .iter()
            .filter(|m| !m.sites.is_empty())
            .collect();
        if !with_sites.is_empty() {
            body.push_str("<h3>Where the heap goes</h3><p>Live allocations while shown, by call site (since recording began).</p>");
            for m in with_sites {
                let _ = write!(
                    body,
                    "<h4><code>{}</code></h4><table><tr><th>Heap</th><th>Allocations</th><th>Where</th></tr>",
                    escape(&m.place())
                );
                for (site, bytes, count) in &m.sites {
                    let _ = write!(
                        body,
                        "<tr><td>{}</td><td>{count}</td><td><code>{}</code></td></tr>",
                        kib(*bytes as f64),
                        escape(site).replace('\n', "<br>")
                    );
                }
                body.push_str("</table>");
            }
        }
    }
    report::page("Budgets on the X4", &body, "")
}

fn table<'a>(body: &mut String, run: &Run, rows: impl Iterator<Item = &'a Measured>) {
    body.push_str(
        "<table><tr><th>Screen or widget</th><th>X4 ms (est.)</th><th>Came up ms</th>\
         <th>Again ms</th><th>Peak heap</th><th>Kept shown</th><th>After closing</th>\
         <th>Calls before</th><th>Calls drawing</th><th>Faults</th></tr>",
    );
    for m in rows {
        let flag = |bad: bool| if bad { " style=\"color:#c00\"" } else { "" };
        let after: Vec<String> = m.after.iter().map(|b| format!("{b:+}")).collect();
        let _ = write!(
            body,
            "<tr><td><code>{}</code>{}</td><td{}>{:.0}</td><td>{:.2}</td><td>{:.2}</td>\
             <td{}>{}</td><td>{}</td><td{}>{} B</td><td{}>{}</td><td>{}</td><td{}>{}</td></tr>",
            escape(&m.place()),
            if m.params.is_empty() {
                String::new()
            } else {
                format!(" <small>{}</small>", escape(&m.params))
            },
            flag(m.x4_ms > FRAME_BUDGET_MS),
            m.x4_ms,
            m.came_up_ms(),
            m.frame_ms(),
            flag(heap_level(run, m).is_some()),
            kib(m.peak as f64),
            kib(m.kept as f64),
            flag(m.leak().is_some()),
            after.join(" "),
            flag(m.before > LOOKUPS_BEFORE),
            m.before,
            m.drawing,
            flag(!m.faults.is_empty()),
            escape(&m.faults.join("; "))
        );
    }
    body.push_str("</table>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_emulator::trace::{Call, Counts};

    fn call(band: usize, op: &'static str, path: &str, bytes: u64) -> Call {
        Call {
            band,
            op,
            path: path.to_string(),
            bytes,
            cost: 0,
        }
    }

    fn frame(calls: Vec<Call>) -> FrameRecord {
        FrameRecord {
            screen: String::from("test"),
            millis: 1.0,
            bands: vec![0; 5],
            drawing: Counts::default(),
            before: Counts::default(),
            calls,
        }
    }

    /// `pictures/a.ppgi` is a picture of 40 000 bytes.
    fn picture(path: &str) -> Option<u64> {
        (path == "pictures/a.ppgi").then_some(40_000)
    }

    #[test]
    fn the_first_band_of_a_new_screen_may_load() {
        let frame = frame(vec![
            call(0, "read", "notes.txt", 100),
            call(0, "list", "notes", 0),
        ]);
        assert!(draw_io(&frame, 1, "came up", &picture).is_empty());
        assert_eq!(
            draw_io(&frame, 0, "drawn again", &picture),
            [
                "drawn again, band 1: read notes.txt",
                "drawn again, band 1: list notes"
            ]
        );
    }

    #[test]
    fn reads_in_later_bands_are_found_once() {
        let frame = frame(vec![
            call(2, "read", "notes.txt", 100),
            call(2, "read", "notes.txt", 100),
            call(3, "exists", "x", 0),
        ]);
        assert_eq!(
            draw_io(&frame, 1, "came up", &picture),
            [
                "came up, band 3: read notes.txt",
                "came up, band 4: exists x"
            ]
        );
    }

    #[test]
    fn a_picture_row_once_a_frame_is_allowed() {
        // 40 000 bytes in five bands, each with its header.
        let rows: Vec<Call> = (0..5)
            .flat_map(|band| {
                [
                    call(band, "size", "pictures/a.ppgi", 0),
                    call(band, "read", "pictures/a.ppgi", 8000 + 16),
                ]
            })
            .collect();
        assert!(draw_io(&frame(rows.clone()), 0, "drawn again", &picture).is_empty());
        // The whole picture read in every band is not.
        let again: Vec<Call> = (0..5)
            .map(|band| call(band, "read", "pictures/a.ppgi", 40_000))
            .collect();
        let found = draw_io(&frame(again), 0, "drawn again", &picture);
        assert_eq!(
            found,
            ["drawn again: 200000 bytes of the picture pictures/a.ppgi of 40000"]
        );
    }

    #[test]
    fn a_leak_is_growth_after_the_first_close() {
        let leaking = Measured {
            after: vec![4000, 4100, 4200, 4300],
            ..Measured::default()
        };
        assert_eq!(leaking.leak(), Some(100));
        let cached = Measured {
            after: vec![4000, 4000, 4000, 4000],
            ..Measured::default()
        };
        assert_eq!(cached.leak(), None);
        let noisy = Measured {
            after: vec![0, 16, 8, 24],
            ..Measured::default()
        };
        assert_eq!(noisy.leak(), None);
    }

    #[test]
    fn the_fastest_frames_count() {
        let m = Measured {
            came_up: vec![2.0, 9.0, 3.0],
            again: vec![1.5, 1.0],
            reference: 0.5,
            ..Measured::default()
        };
        assert_eq!(m.came_up_ms(), 2.0);
        assert_eq!(m.frame_ms(), 1.0);
        assert_eq!(m.worst_ms(), 2.0);
        // The computer was half as fast as when calibrated: frames count
        // half; never more than twice either way.
        assert_eq!(m.scale(0.25), 0.5);
        assert_eq!(m.scale(10.0), 2.0);
        assert!(Measured::default().worst_ms().is_nan());
        assert_eq!(Measured::default().scale(1.0), 1.0);
    }
}
