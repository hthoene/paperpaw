//! What `perf` looks for, it finds: a test plugin with a screen and a
//! widget that read while they draw (as screens once did, see
//! `crates/paperpaw-app/tests/draw.rs`), a screen too large for the X4's
//! heap and one that leaks a little each time it opens.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Place, Turn};
use std::sync::Mutex;

use paperpaw_core::kit::widgets::{Drawn, WidgetSpec};
use paperpaw_core::{KernelBuilder, Staging};
use paperpaw_dev::cli::Context;
use paperpaw_dev::perf;
use paperpaw_dev::report::{Level, Report};
use paperpaw_dev::target::Target;
use paperpaw_sdk::{Canvas, Handled, Id, Manifest, RegisterError, SDK_VERSION};

/// Reads a file whenever it is drawn.
struct Reads;

impl paperpaw_core::testing::Fake for Reads {
    fn draw(&mut self, _: &mut Canvas<'_>, env: &Place<'_>) {
        let _ = paperpaw_sdk::read_file(env.storage(), "notes.txt", 1024);
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// Keeps 200 KiB while it is open: more than the X4 has free.
struct Big(
    #[allow(
        dead_code,
        reason = "shared by the tests; not every test file uses every helper"
    )]
    Vec<u8>,
);

impl paperpaw_core::testing::Fake for Big {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// What the leaking screen keeps, never let go.
static LEAKED: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

/// The heap counter `perf` reads is the process's: tests measuring at the same
/// time count each other's allocations as leaks, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Leaks;

impl paperpaw_core::testing::Fake for Leaks {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

struct Probe;

impl Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("perf-probe"), "Perf probe", SDK_VERSION)
            .description("Screens that break the X4's budgets.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Staging) -> Result<(), RegisterError> {
        r.add_fake(paperpaw_core::testing::FakeSpec::new(
            Id::new_static("perf-probe.reads"),
            "perf-probe.reads",
            Box::new(|_, _| Box::new(Reads)),
        ))?;
        r.add_fake(paperpaw_core::testing::FakeSpec::new(
            Id::new_static("perf-probe.big"),
            "perf-probe.big",
            Box::new(|_, _| Box::new(Big(vec![1; 200 * 1024]))),
        ))?;
        r.add_fake(paperpaw_core::testing::FakeSpec::new(
            Id::new_static("perf-probe.leaks"),
            "perf-probe.leaks",
            Box::new(|_, _| {
                LEAKED.lock().unwrap().push(vec![1; 1024]);
                Box::new(Leaks)
            }),
        ))?;
        // A widget reading a file whenever it is drawn.
        r.add_widget(WidgetSpec::drawn(
            Id::new_static("perf-probe.widget"),
            "perf-probe.widget",
            Drawn::new(
                |_, _| 1,
                |_, env, _| {
                    let _ = paperpaw_sdk::read_file(env.storage(), "notes.txt", 1024);
                },
            ),
        ))
    }
}

fn add(builder: KernelBuilder) -> KernelBuilder {
    builder.items(Probe.manifest(), |r| Probe.register(r))
}

/// Runs `perf` over the test plugin, on the X4.
fn probe_report(name: &str) -> (Context, Report) {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-perf-{name}-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec!["perf-probe".into()],
            add: Some(add),
            ..Target::default()
        },
        out,
    );
    context.quick = true;
    let mut report = Report::default();
    perf::run(&context, &mut report).unwrap();
    (context, report)
}

fn found(report: &Report, check: &str, place: &str, level: Level) -> Vec<String> {
    report
        .findings
        .iter()
        .filter(|f| f.check == check && f.place == place && f.level == level)
        .map(|f| f.message.clone())
        .collect()
}

/// A screen that reads the card while it draws (as the sleep screens of
/// stats, news and notes did, #4357) is an error: the guard of the standard
/// app and this check count the same calls.
#[test]
fn reads_while_drawing_large_screens_and_leaks_are_found() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (context, report) = probe_report("screens");
    let text = report.text(Level::Info);

    // The file is not there: reading it asks its size, in vain.
    let asked = |m: &String| m.contains("drawn again, band 1: size notes.txt");
    let reads = found(&report, "perf.draw-io", "perf-probe.reads", Level::Error);
    assert!(reads.iter().any(asked), "{text}");
    assert!(
        !found(&report, "perf.heap", "perf-probe.big", Level::Error).is_empty(),
        "{text}"
    );
    assert!(
        !found(&report, "perf.leak", "perf-probe.leaks", Level::Warning).is_empty(),
        "{text}"
    );
    // Nothing else is wrong with them.
    for place in ["perf-probe.big", "perf-probe.leaks"] {
        assert!(
            found(&report, "perf.draw-io", place, Level::Error).is_empty(),
            "{text}"
        );
    }
    assert!(
        found(&report, "perf.heap", "perf-probe.reads", Level::Error).is_empty(),
        "{text}"
    );
    assert!(
        found(&report, "perf.leak", "perf-probe.reads", Level::Warning).is_empty(),
        "{text}"
    );
    assert!(context.out.join("perf/index.html").is_file());
    assert_eq!(report.pages[0].title, "Budgets on the X4");
    let _ = std::fs::remove_dir_all(&context.out);
}

#[test]
#[ignore = "the dev tool measures a widget's cell with its interface-6 probe widget, which the native Home does not place (todo 634, #244)"]
fn a_widget_that_reads_while_drawing_is_found() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (context, report) = probe_report("widget");
    let text = report.text(Level::Info);
    let asked = |m: &String| m.contains("drawn again, band 1: size notes.txt");
    let widget = found(&report, "perf.draw-io", "perf-probe.widget", Level::Error);
    assert!(widget.iter().any(asked), "{text}");
    let _ = std::fs::remove_dir_all(&context.out);
}

/// A store plugin ships switched off; a check that names it looks at it
/// switched on, so that its screens are there to be measured (they were
/// not: "0 screens", and what a plugin read while it drew was not found).
#[test]
fn a_store_plugin_named_is_switched_on() {
    let context = |plugin: &str| {
        Context::new(
            Target {
                plugins: vec![plugin.into()],
                ..Target::default()
            },
            std::env::temp_dir().join("paperpaw-dev-switched-on"),
        )
    };
    for plugin in ["stats", "mail", "browser", "wikipedia", "format-rtf"] {
        assert_eq!(
            paperpaw_dev::session::switched_on(&context(plugin)),
            [(format!("plugins.{plugin}"), "true".to_string())],
            "{plugin}"
        );
    }
    // A plugin that ships on is left as it is; so is the whole app.
    assert!(paperpaw_dev::session::switched_on(&context("home")).is_empty());
    assert!(paperpaw_dev::session::switched_on(&context("keyboard")).is_empty());
    let all = Context::new(Target::default(), std::env::temp_dir());
    assert!(paperpaw_dev::session::switched_on(&all).is_empty());
}

/// The dev session shows a screen and tells it so (`ScreenShown`) after its
/// first frame. A page laid out in the draw then laid itself out anew in the
/// second frame, and what the widgets it makes ask of the host (the reading
/// goal's cat reads the registry's plugin list, which reads the card's record
/// of the store's installs) was asked while drawing (#450). Picture home lays
/// the page out when it is shown, so its draw asks the card nothing.
#[test]
fn picture_home_draws_without_asking_the_card_when_it_is_shown_after_its_first_frame() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = std::env::temp_dir().join(format!("paperpaw-dev-perf-shown-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec!["picture-home".into()],
            ..Target::default()
        },
        out,
    );
    context.quick = true;
    let mut report = Report::default();
    perf::run(&context, &mut report).unwrap();
    let text = report.text(Level::Info);
    assert!(
        text.contains("picture-home.dashboard"),
        "not measured:\n{text}"
    );
    assert!(
        found(
            &report,
            "perf.draw-io",
            "picture-home.dashboard",
            Level::Error
        )
        .is_empty(),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&context.out);
}

/// Every plugin of the app can be named: the ones that were left out of the
/// dev app have their screens now, and a store plugin has them switched on.
#[test]
fn the_plugins_once_left_out_have_screens() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-perf-left-{}", std::process::id()));
    let target = Target {
        plugins: ["browser", "wikipedia", "kosync", "format-rtf", "stats"]
            .map(String::from)
            .to_vec(),
        ..Target::default()
    };
    let context = Context::new(target.clone(), out);
    let session =
        paperpaw_dev::session::Session::new(&context, &paperpaw_dev::variant::Variant::x4())
            .unwrap();
    let screens = paperpaw_dev::cases::screens(&session, &target);
    for wanted in ["browser.screen", "wikipedia.lookup", "stats.overview"] {
        assert!(
            screens.iter().any(|s| s == wanted),
            "{wanted} missing from {screens:?}"
        );
    }
}

/// Screens that no parameters open are measured by a way in declared for
/// them (a question asked, the phone keyboard) or with parameters that open
/// them (a secret of the settings), none is left out as "not measured"; the
/// one that needs a radio the X4 lacks is a note with its reason.
#[test]
fn screens_that_need_a_way_in_are_measured() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = std::env::temp_dir().join(format!("paperpaw-dev-perf-ways-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: ["keyboard", "transfer", "bluetooth", "settings"]
                .map(String::from)
                .to_vec(),
            ..Target::default()
        },
        out,
    );
    context.quick = true;
    let mut report = Report::default();
    perf::run(&context, &mut report).unwrap();
    let skipped: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.check == "perf.skipped")
        .map(|f| (f.level, f.place.clone()))
        .collect();
    assert_eq!(
        skipped,
        [(Level::Info, "bluetooth.search".to_string())],
        "{}",
        report.text(Level::Info)
    );
    // They were measured: each has its line among the budgets.
    let page = std::fs::read_to_string(context.out.join("perf/index.html")).unwrap();
    for screen in ["keyboard.ask", "transfer.phone", "settings.secret"] {
        assert!(page.contains(screen), "{screen} not measured");
    }
    let _ = std::fs::remove_dir_all(&context.out);
}

/// The store's list starts a fetch of the list each time it opens: the
/// request left under way when it closes is not heap kept across openings
/// (#330), so `perf` does not call it a leak.
#[test]
fn a_request_left_under_way_is_not_a_leak_of_the_stores_list() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = std::env::temp_dir().join(format!("paperpaw-dev-perf-store-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec!["store".into()],
            ..Target::default()
        },
        out,
    );
    context.quick = true;
    // Answered from the fixtures: the computer's network answers in its own
    // time, which made this test depend on the machine it ran on.
    context.fixtures = true;
    let mut report = Report::default();
    perf::run(&context, &mut report).unwrap();
    let leaks: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.check == "perf.leak" && f.place == "store.list")
        .map(|f| f.message.clone())
        .collect();
    assert!(leaks.is_empty(), "{leaks:?}");
    let _ = std::fs::remove_dir_all(&context.out);
}

/// The firmware confirmation guards the device: opening the home slot
/// leaves it open, and every screen measured after it opened below it,
/// unseen, until the stack was full (#336). It is closed before the next
/// screen, and a screen not measured is an error naming it.
#[test]
fn screens_after_a_guarding_screen_are_measured() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = std::env::temp_dir().join(format!("paperpaw-dev-perf-guard-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec!["core".into()],
            ..Target::default()
        },
        out,
    );
    context.quick = true;
    let mut report = Report::default();
    perf::run(&context, &mut report).unwrap();
    let skipped: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.check == "perf.skipped")
        .map(|f| (f.level, f.place.clone()))
        .collect();
    assert!(skipped.is_empty(), "{skipped:?}");
    let _ = std::fs::remove_dir_all(&context.out);
}
