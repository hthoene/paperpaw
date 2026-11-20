//! Snapshots: screens and widgets drawn on emulated devices and compared
//! with reference pictures, `<snapshots dir>/<name>.png`; written anew
//! with `--bless` (or `PAPERPAW_BLESS=1`). A few pixels may differ, where
//! glyph edges round differently between computers, as in the emulator's
//! own snapshot test (`shells/desktop/tests/snapshots.rs`); more fails,
//! and the report shows the expected picture, the actual one and where
//! they differ.
//!
//! The cases come from `<snapshots dir>/cases.ini` if there is one:
//!
//! ```ini
//! # A section a case, its name that of the picture.
//! [x4-hello]
//! screen = hello.screen
//! profile = xteink-x4
//! # Script steps after it is shown, as `paperpaw-emu --script` takes them.
//! script = down,confirm
//! # Parameters of the screen.
//! param.book = books/alice-in-wonderland.epub
//! # A file put on the card before the start: `<card path> <- <file in
//! # data/ beside cases.ini>` (the data a screen shows, kept as its plugin
//! # keeps it).
//! file = .paperpaw/plugins/tasks/tasks.txt <- tasks.txt
//! # Keys with a dot are settings.
//! core.night-mode = true
//!
//! [boox-clock-wide]
//! widget = widgets.analogue-clock 4x2
//! profile = boox-go-7
//!
//! # Neither a screen nor a widget: the home screen.
//! [x4-home]
//! profile = xteink-x4
//! ```
//!
//! Otherwise every screen and widget of the plugins looked at, widgets in
//! a few sizes, on the X4 and the Boox (or the profiles asked for); a data
//! plugin without either gets the home screen.
//!
//! A plugin's repository runs its cases as a test with [`check_dir`],
//! the cases of `cases.ini` (see `tools/dev/templates/tests/snapshots.rs`),
//! or [`check`] with cases in code ([`Case::screen`], [`Case::widget`]),
//! their pictures best in a folder of their own: `paperpaw-dev check`
//! looks at `tests/snapshots` with the cases of its `cases.ini`, or every
//! screen and widget where there is none.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use paperpaw_emulator::profiles;

use crate::AddPlugins;
use crate::cases;
use crate::cli::Context;
use crate::image::Image;
use crate::report::{self, Finding, Level, Report};
use crate::session::{Session, Start};
use crate::target::Target;
use crate::variant::{self, Variant};

/// A channel differing by more than this counts the pixel as different.
pub const LEVELS: u8 = 48;
/// The share of pixels that may differ, in parts per ten thousand.
pub const ALLOWED: usize = 20;
/// The file in the snapshots folder that lists the cases.
pub const CASES_FILE: &str = "cases.ini";
/// The environment variable that writes the references anew.
pub const BLESS: &str = "PAPERPAW_BLESS";

/// What a case shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// A screen, opened with parameters as a plugin would.
    Screen {
        id: String,
        params: Vec<(String, String)>,
    },
    /// A widget alone on the home screen, in columns and rows of the
    /// grid; the picture is its cell.
    Widget { id: String, size: (u8, u8) },
    /// The home screen.
    Home,
}

/// One picture to compare: what is shown, where, how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// The picture's name, `<name>.png`; made from the rest if empty.
    pub name: String,
    pub subject: Subject,
    /// A device profile (`xteink-x4`, `boox-go-7`, see `--list`).
    pub profile: String,
    /// Settings at start, as `(id, value)`.
    pub settings: Vec<(String, String)>,
    /// Script steps after it is shown (`down,confirm,wait:300`).
    pub script: String,
    /// Files put on the card before the start, as `(path on the card, file
    /// in `data/` beside `cases.ini`)`.
    pub files: Vec<(String, String)>,
}

impl Case {
    fn of(subject: Subject) -> Case {
        Case {
            name: String::new(),
            subject,
            profile: String::from("xteink-x4"),
            settings: Vec::new(),
            script: String::new(),
            files: Vec::new(),
        }
    }

    /// The screen `id`, on the X4 unless [`on`](Self::on) says otherwise.
    pub fn screen(id: impl Into<String>) -> Case {
        Self::of(Subject::Screen {
            id: id.into(),
            params: Vec::new(),
        })
    }

    /// The widget `id` in `size` (columns, rows of the grid).
    pub fn widget(id: impl Into<String>, size: (u8, u8)) -> Case {
        Self::of(Subject::Widget {
            id: id.into(),
            size,
        })
    }

    /// The home screen.
    pub fn home() -> Case {
        Self::of(Subject::Home)
    }

    /// On the device profile `profile`.
    pub fn on(mut self, profile: impl Into<String>) -> Case {
        self.profile = profile.into();
        self
    }

    /// With setting `id` set to `value` from the start.
    pub fn set(mut self, id: impl Into<String>, value: impl Into<String>) -> Case {
        self.settings.push((id.into(), value.into()));
        self
    }

    /// A screen opened with parameter `key`.
    pub fn param(mut self, key: impl Into<String>, value: impl Into<String>) -> Case {
        if let Subject::Screen { params, .. } = &mut self.subject {
            params.push((key.into(), value.into()));
        }
        self
    }

    /// Script steps after it is shown.
    pub fn script(mut self, steps: impl Into<String>) -> Case {
        self.script = steps.into();
        self
    }

    /// Named `name` (its picture `name.png`).
    pub fn named(mut self, name: impl Into<String>) -> Case {
        self.name = name.into();
        self
    }

    /// Its name: given, or the profile and what it shows
    /// (`xteink-x4-hello-screen`, `boox-go-7-widgets-clock-2x1`).
    pub fn file_name(&self) -> String {
        if !self.name.is_empty() {
            return slug(&self.name);
        }
        let what = match &self.subject {
            Subject::Screen { id, .. } => id.clone(),
            Subject::Widget { id, size } => format!("{id}-{}x{}", size.0, size.1),
            Subject::Home => String::from("home"),
        };
        slug(&format!("{}-{what}", self.profile))
    }

    /// The id of what it shows, for the plugin it belongs to.
    fn id(&self) -> Option<&str> {
        match &self.subject {
            Subject::Screen { id, .. } | Subject::Widget { id, .. } => Some(id),
            Subject::Home => None,
        }
    }
}

/// `text` as a file name: lower-case letters, digits and `-`.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The cases of a `cases.ini` (see the [module documentation](self)).
pub fn read_cases(source: &str) -> Result<Vec<Case>, String> {
    let mut out: Vec<Case> = Vec::new();
    let mut section = "";
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(|e| format!("{CASES_FILE}: {e}"))?;
        if entry.section.is_empty() {
            return Err(format!(
                "{CASES_FILE}:{}: `{}` is outside a case; start one with [name]",
                entry.line, entry.key
            ));
        }
        if entry.section != section {
            section = entry.section;
            out.push(Case::home().named(section));
        }
        let Some(case) = out.last_mut() else {
            continue;
        };
        let value = entry.value.as_str().trim().to_string();
        let at = |what: &str| format!("{CASES_FILE}:{}: {what}", entry.line);
        match entry.key {
            "screen" => {
                case.subject = Subject::Screen {
                    id: value,
                    params: Vec::new(),
                }
            }
            "widget" => {
                let (id, size) = value
                    .split_once(' ')
                    .ok_or_else(|| at("a widget is `widget = <id> <columns>x<rows>`"))?;
                let size = size
                    .trim()
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .filter(|&(w, h): &(u8, u8)| w > 0 && h > 0)
                    .ok_or_else(|| at("the size is like 2x1"))?;
                case.subject = Subject::Widget {
                    id: id.to_string(),
                    size,
                };
            }
            "profile" => {
                if profiles::find(&value).is_none() {
                    return Err(at(&format!("unknown profile `{value}` (see --list)")));
                }
                case.profile = value;
            }
            "script" => case.script = value,
            "file" => {
                let (path, name) = value
                    .split_once(" <- ")
                    .ok_or_else(|| at("a file is `file = <card path> <- <file in data/>`"))?;
                case.files
                    .push((path.trim().to_string(), name.trim().to_string()));
            }
            key if key.starts_with("param.") => {
                let key = key.trim_start_matches("param.").to_string();
                match &mut case.subject {
                    Subject::Screen { params, .. } => params.push((key, value)),
                    _ => return Err(at("parameters are for screens, after `screen =`")),
                }
            }
            key if key.contains('.') => case.settings.push((key.to_string(), value)),
            key => return Err(at(&format!("`{key}` is not read in a case"))),
        }
    }
    Ok(out)
}

/// The cases when none are given: every screen and widget of the target,
/// widgets in a few sizes, on each profile (the X4 and the Boox unless
/// others were asked for); the home screen if there are neither.
pub fn default_cases(context: &Context) -> Result<Vec<Case>, String> {
    let asked: Vec<&str> = context.profiles.iter().map(|p| p.device.id).collect();
    let profiles: Vec<&str> = if asked == variant::DEFAULT_PROFILES {
        vec!["xteink-x4", "boox-go-7"]
    } else {
        asked
    };
    let mut out = Vec::new();
    for profile in profiles {
        let Some(found) = profiles::find(profile) else {
            continue;
        };
        let session = Session::new(context, &Variant::of(found))?;
        let screens = cases::screens(&session, &context.target);
        let widgets = cases::widgets(&session, &context.target);
        let grid = cases::grid(&session);
        let mut sizes = vec![(1, 1), (2, 1), (2, 2), (grid.0, 2)];
        sizes.retain(|&(w, h)| w <= grid.0 && h <= grid.1);
        sizes.dedup();
        for id in &screens {
            out.push(Case::screen(id.as_str()).on(profile));
        }
        for (id, _) in &widgets {
            for &size in &sizes {
                out.push(Case::widget(id.as_str(), size).on(profile));
            }
        }
        if screens.is_empty() && widgets.is_empty() {
            out.push(Case::home().on(profile));
        }
    }
    Ok(out)
}

/// What a comparison came to.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub matched: Vec<String>,
    pub written: Vec<String>,
    /// Differing, missing or not drawn.
    pub failed: Vec<String>,
}

/// A session kept for the next case on the same device with the same
/// settings.
struct Running {
    profile: String,
    settings: Vec<(String, String)>,
    files: Vec<(String, String)>,
    session: Session,
}

/// The `snapshots` command: the cases of `cases.ini` in the snapshots
/// folder (`--snapshots`, else `tests/snapshots`), or the default ones.
pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let dir = context
        .snapshots
        .clone()
        .unwrap_or_else(|| PathBuf::from("tests/snapshots"));
    let file = dir.join(CASES_FILE);
    let cases = if file.is_file() {
        let source =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        read_cases(&source)?
    } else {
        default_cases(context)?
    };
    let bless = context.bless || blessing();
    compare(context, &cases, &dir, bless, report).map(|_| ())
}

/// Whether `PAPERPAW_BLESS` asks for the references to be written.
pub fn blessing() -> bool {
    std::env::var(BLESS).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Draws every case and compares it with `<dir>/<name>.png` (writes it
/// if `bless`); findings and a page with the differences into `report`.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn compare(
    context: &Context,
    cases: &[Case],
    dir: &Path,
    bless: bool,
    report: &mut Report,
) -> Result<Outcome, String> {
    let mut outcome = Outcome::default();
    let mut names: Vec<String> = Vec::new();
    for case in cases {
        let name = case.file_name();
        if name.is_empty() || names.contains(&name) {
            return Err(format!(
                "two snapshot cases are named `{name}`: give one a name of its own"
            ));
        }
        names.push(name);
    }
    let out_dir = PathBuf::from("snapshots");
    let mut sheet = String::new();
    // Cases without a script on the same device with the same settings
    // share a session.
    let mut current: Option<Running> = None;
    for case in cases {
        let name = case.file_name();
        let place = format!("{name}.png");
        let plugin = case
            .id()
            .and_then(|id| context.target.owner(id))
            .map(str::to_string);
        let with = |finding: Finding| match &plugin {
            Some(p) => finding.plugin(p.clone()),
            None => finding,
        };
        let mut settings = context.settings.clone();
        settings.extend(case.settings.iter().cloned());
        let reuse = current.as_ref().is_some_and(|r| {
            r.profile == case.profile && r.settings == settings && r.files == case.files
        }) && case.script.is_empty();
        if !reuse {
            // The session before goes first, as one device runs at a time.
            drop(current.take());
            let profile = profiles::find(&case.profile)
                .ok_or_else(|| format!("{name}: unknown profile `{}`", case.profile))?;
            let mut files = Vec::new();
            for (path, name) in &case.files {
                let source = dir.join("data").join(name);
                let bytes = std::fs::read(&source)
                    .map_err(|e| format!("{name}: {}: {e}", source.display()))?;
                files.push((path.clone(), bytes));
            }
            let start = Start {
                settings: settings.clone(),
                files,
                ..Start::default()
            };
            let session = Session::start(context, &Variant::of(profile), &start)?;
            current = Some(Running {
                profile: case.profile.clone(),
                settings,
                files: case.files.clone(),
                session,
            });
        }
        let Some(Running { session, .. }) = current.as_mut() else {
            continue;
        };
        let actual = match draw(session, case) {
            Ok(image) => image,
            Err(problem) => {
                report.add(with(Finding::error("snapshots.case", place, problem)));
                outcome.failed.push(name);
                current = None;
                continue;
            }
        };
        if !case.script.is_empty() {
            current = None;
        }
        let reference = dir.join(&place);
        if bless {
            actual.save_png(&reference)?;
            report.add(with(Finding::info(
                "snapshots.written",
                place,
                format!(
                    "Written to {}; look at it before committing it.",
                    reference.display()
                ),
            )));
            outcome.written.push(name);
            continue;
        }
        let actual_path = out_dir.join(format!("{name}.actual.png"));
        if !reference.is_file() {
            actual.save_png(&context.out.join(&actual_path))?;
            report.add(with(
                Finding::error(
                    "snapshots.missing",
                    place,
                    format!(
                        "No reference picture {}; write it with --bless (or {BLESS}=1) and look at it before committing it.",
                        reference.display()
                    ),
                )
                .picture(actual_path),
            ));
            outcome.failed.push(name);
            continue;
        }
        let expected = Image::load_png(&reference)?;
        let differing = expected.differing(&actual, LEVELS);
        let allowed = (actual.width * actual.height) as usize * ALLOWED / 10_000;
        if differing <= allowed {
            outcome.matched.push(name);
            continue;
        }
        let expected_path = out_dir.join(format!("{name}.expected.png"));
        let diff_path = out_dir.join(format!("{name}.diff.png"));
        expected.save_png(&context.out.join(&expected_path))?;
        actual.save_png(&context.out.join(&actual_path))?;
        difference(&expected, &actual).save_png(&context.out.join(&diff_path))?;
        let size_note = if (expected.width, expected.height) != (actual.width, actual.height) {
            format!(
                " (the reference is {}x{}, the picture {}x{})",
                expected.width, expected.height, actual.width, actual.height
            )
        } else {
            String::new()
        };
        report.add(with(
            Finding::error(
                "snapshots.differs",
                place,
                format!(
                    "{differing} pixels differ from the reference, {allowed} may{size_note}; if the change is meant, write it anew with --bless."
                ),
            )
            .picture(diff_path.clone()),
        ));
        let _ = write!(sheet, "<h3>{}</h3><div class=sheet>", report::escape(&name));
        for (title, path) in [
            ("expected", &expected_path),
            ("actual", &actual_path),
            ("difference", &diff_path),
        ] {
            let _ = write!(
                sheet,
                "<figure><img src=\"{}\" width={}><figcaption>{title}</figcaption></figure>",
                report::escape(&report::slashed(path)),
                actual.width.min(360)
            );
        }
        sheet.push_str("</div>");
        outcome.failed.push(name);
    }
    if !sheet.is_empty() {
        let body = format!(
            "<h1>Snapshots that differ</h1><p>Expected (the reference in {}), actual, and the pixels that differ in red.</p>{sheet}",
            report::escape(&dir.display().to_string())
        );
        let path = out_dir.join("index.html");
        let full = context.out.join(&path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        // The pictures are beside the page.
        let body = body.replace("src=\"snapshots/", "src=\"");
        std::fs::write(&full, report::page("Snapshots", &body, ""))
            .map_err(|e| format!("{}: {e}", full.display()))?;
        report.page("Snapshots that differ", path);
    }
    if !outcome.matched.is_empty() {
        report.add(Finding::info(
            "snapshots.matched",
            dir.display().to_string(),
            format!(
                "{} of {} pictures match their references.",
                outcome.matched.len(),
                cases.len()
            ),
        ));
    }
    Ok(outcome)
}

/// Shows the case on the session and returns the picture.
fn draw(session: &mut Session, case: &Case) -> Result<Image, String> {
    match &case.subject {
        Subject::Screen { id, params } => {
            cases::home(session);
            let params: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let shown = cases::show_screen(session, id, &params)?;
            if !shown.opened() {
                return Err(format!(
                    "The screen {id} did not open (shown: {}); does it need parameters (param.<key> in cases.ini)?",
                    shown.shown.as_deref().unwrap_or("nothing")
                ));
            }
            session.script(&case.script)?;
            Ok(session.image())
        }
        Subject::Widget { id, size } => {
            let shown = cases::show_widget(session, id, *size)?;
            let Some(cell) = shown.cell else {
                return Err(format!(
                    "The widget {id} was not placed in {}x{}: is the grid smaller?",
                    size.0, size.1
                ));
            };
            session.script(&case.script)?;
            Ok(session.image().crop(cell))
        }
        Subject::Home => {
            cases::home(session);
            session.script(&case.script)?;
            Ok(session.image())
        }
    }
}

/// The expected picture faded, the pixels that differ in red; the actual
/// picture framed in red if the sizes differ.
pub fn difference(expected: &Image, actual: &Image) -> Image {
    const RED: [u8; 3] = [220, 0, 0];
    if (expected.width, expected.height) != (actual.width, actual.height) {
        let mut framed = actual.clone();
        framed.outline(
            paperpaw_platform::Rect::new(0, 0, actual.width as u16, actual.height as u16),
            3,
            RED,
        );
        return framed;
    }
    let mut out = Image::filled(actual.width, actual.height, [255, 255, 255]);
    for y in 0..actual.height {
        for x in 0..actual.width {
            let (a, b) = (expected.pixel(x, y), actual.pixel(x, y));
            let differs = a.iter().zip(b.iter()).any(|(p, q)| p.abs_diff(*q) > LEVELS);
            let rgb = if differs {
                RED
            } else {
                let faded = 255 - (255 - expected.luma(x, y)) / 4;
                [faded; 3]
            };
            out.set_pixel(x, y, rgb);
        }
    }
    out
}

/// Runs `cases` against the pictures in `dir` for a plugin's own test:
/// the app with the plugins `add` adds, the references written anew with
/// `PAPERPAW_BLESS=1`. The report goes to `target/paperpaw-dev/test`;
/// an error names every case that failed.
///
/// ```no_run
/// use paperpaw_dev::snapshot::{self, Case};
///
/// # fn add(app: paperpaw_core::KernelBuilder) -> paperpaw_core::KernelBuilder { app }
/// snapshot::check(add, "tests/snapshots", &[
///     Case::screen("hello.screen"),
///     Case::screen("hello.screen").on("boox-go-7").set("core.night-mode", "true"),
/// ])
/// .unwrap();
/// ```
pub fn check(add: AddPlugins, dir: impl AsRef<Path>, cases: &[Case]) -> Result<Outcome, String> {
    let out = PathBuf::from("target/paperpaw-dev/test");
    check_into(add, dir.as_ref(), cases, &out, blessing())
}

/// [`check`] with the cases of `<dir>/cases.ini`: the same cases
/// `paperpaw-dev check` runs from that folder, so the test and the
/// command line never disagree on what the pictures are.
///
/// ```no_run
/// # fn add(app: paperpaw_core::KernelBuilder) -> paperpaw_core::KernelBuilder { app }
/// if let Err(problems) = paperpaw_dev::snapshot::check_dir(add, "tests/snapshots") {
///     panic!("{problems}");
/// }
/// ```
pub fn check_dir(add: AddPlugins, dir: impl AsRef<Path>) -> Result<Outcome, String> {
    let dir = dir.as_ref();
    let file = dir.join(CASES_FILE);
    let source = std::fs::read_to_string(&file).map_err(|e| {
        format!(
            "{}: {e}; list the cases there (see paperpaw_dev::snapshot)",
            file.display()
        )
    })?;
    check(add, dir, &read_cases(&source)?)
}

/// [`check`] with its output folder and blessing given.
pub fn check_into(
    add: AddPlugins,
    dir: &Path,
    cases: &[Case],
    out: &Path,
    bless: bool,
) -> Result<Outcome, String> {
    let context = Context::new(
        Target {
            add: Some(add),
            ..Target::default()
        },
        out.to_path_buf(),
    );
    let mut report = Report::default();
    report.ran.push(String::from("snapshots"));
    let outcome = compare(&context, cases, dir, bless, &mut report)?;
    report.write(out)?;
    if outcome.failed.is_empty() {
        return Ok(outcome);
    }
    Err(format!(
        "{}see {}",
        report.text(Level::Warning),
        out.join("report.html").display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_are_read_and_named() {
        let cases = read_cases(
            "[x4-hello]\nscreen = hello.screen\nscript = down\nparam.book = a.epub\ncore.night-mode = true\n\
             [clock]\nwidget = widgets.analogue-clock 2x1\nprofile = boox-go-7\n[home]\n",
        );
        // A section with nothing in it is no entry of the file.
        let cases = cases.unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(
            cases[0].subject,
            Subject::Screen {
                id: "hello.screen".into(),
                params: vec![("book".into(), "a.epub".into())]
            }
        );
        assert_eq!(
            cases[0].settings,
            [("core.night-mode".into(), "true".into())]
        );
        assert_eq!(cases[0].file_name(), "x4-hello");
        assert_eq!(cases[1].profile, "boox-go-7");
        assert_eq!(
            Case::widget("widgets.clock", (2, 1))
                .on("boox-go-7")
                .file_name(),
            "boox-go-7-widgets-clock-2x1"
        );
        assert!(read_cases("[a]\nwidget = x 2y1\n").is_err());
        assert!(read_cases("[a]\nprofile = nothing\n").is_err());
        assert!(read_cases("[a]\nwhat = x\n").is_err());
    }

    #[test]
    fn differences_are_red() {
        let a = Image::filled(4, 2, [255, 255, 255]);
        let mut b = a.clone();
        b.set_pixel(1, 1, [0, 0, 0]);
        let diff = difference(&a, &b);
        assert_eq!(diff.pixel(1, 1), [220, 0, 0]);
        assert_eq!(diff.pixel(0, 0), [255, 255, 255]);
        let other = Image::filled(3, 3, [0, 0, 0]);
        assert_eq!(difference(&a, &other).pixel(0, 0), [220, 0, 0]);
    }
}
