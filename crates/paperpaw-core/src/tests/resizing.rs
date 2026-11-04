//! Screens fitted to another size: laid out anew where they can
//! (`Screen::on_resize`), else closed, dropped and made anew; items newer
//! than a screen's revision never called on it, also through a host.

use super::*;
use paperpaw_sdk::Pane;
use paperpaw_sdk::host::ScreenHost;
use paperpaw_sdk::{Params, SCREEN_REVISION};

const FIT: Id = Id::new_static("fit.screen");
const HOST: Id = Id::new_static("fit.host");

/// How the plugin's screens are built: the revision they say (1 as a
/// screen built before revision 2's items), whether they lay themselves
/// out anew.
#[derive(Clone, Copy)]
struct Build {
    revision: u16,
    takes: bool,
}

/// Logs what is asked of it, its size included, and when it goes.
struct Fitting {
    log: Log,
    build: Build,
}

impl Screen for Fitting {
    // A screen built before: what plugins never write (see the docs).
    fn revision(&self) -> u16 {
        self.build.revision
    }

    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_close(&mut self, _: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("close"));
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let content = cx.env().content();
        self.log.borrow_mut().push(alloc::format!(
            "resize {}x{}",
            content.width,
            content.height
        ));
        if self.build.takes {
            Handled::Yes
        } else {
            // Made anew instead: what it asks for here is never done.
            cx.command(Command::Notify(String::from("not kept")));
            Handled::No
        }
    }

    fn pane(&self) -> Pane {
        self.log.borrow_mut().push(String::from("pane"));
        Pane::DETAIL
    }
}

impl Drop for Fitting {
    fn drop(&mut self) {
        self.log.borrow_mut().push(String::from("drop"));
    }
}

/// Holds the fitting screen in all of its part, and lays it out anew
/// with itself.
struct Host {
    host: ScreenHost,
}

impl Host {
    fn hold(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let content = cx.env().content();
        let all = Rect::new(0, 0, content.width, content.height);
        self.host.show(cx, FIT, Params::new(), all)
    }
}

impl Screen for Host {
    fn revision(&self) -> u16 {
        self.host.revision()
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.host.draw(canvas, env);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        assert!(self.hold(cx));
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        if self.hold(cx) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}

/// The fitting screen, and the host, one of them in the home slot.
struct FitPlugin {
    log: Log,
    build: Build,
    home: Id,
}

impl Plugin for FitPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("fit"), "Fit", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (log, build) = (self.log.clone(), self.build);
        r.add_screen(ScreenSpec::new(
            FIT,
            "fit",
            Box::new(move |env, _| {
                let display = env.device().display;
                log.borrow_mut()
                    .push(alloc::format!("made {}x{}", display.width, display.height));
                Box::new(Fitting {
                    log: log.clone(),
                    build,
                })
            }),
        ))?;
        r.add_screen(ScreenSpec::new(
            HOST,
            "host",
            Box::new(|_, _| {
                Box::new(Host {
                    host: ScreenHost::new(),
                })
            }),
        ))?;
        r.bind_slot(slots::HOME, self.home.clone(), Priority::DEFAULT)
    }
}

fn fit_kernel(log: &Log, revision: u16, takes: bool, home: Id) -> Kernel {
    let build = Build { revision, takes };
    let plugin = FitPlugin {
        log: log.clone(),
        build,
        home,
    };
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: super::log() })
        .plugin(plugin)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

/// A screen that lays itself out anew is kept: asked with its new size,
/// neither closed nor made anew, and drawn at that size.
#[test]
fn a_screen_that_lays_itself_out_anew_is_kept() {
    let log = log();
    let mut kernel = fit_kernel(&log, SCREEN_REVISION, true, FIT);
    let effects = kernel.resize(Instant(100), 120, 60);
    let area = kernel.stack[0].area;
    assert_eq!((area.width, area.height), (120, 60));
    assert_eq!(*log.borrow(), ["resize 120x60"]);
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
    let mut size = (0, 0);
    kernel.render(&mut |_, rows| size = (rows.width, size.1 + rows.height));
    assert_eq!(size, (120, 60));
}

/// One that does not is closed where it was, dropped, and only then made
/// anew: the two never take memory at once. Made anew, it is asked how
/// it shows where the screen is wide.
#[test]
fn a_screen_made_anew_goes_before_the_new_one_is_made() {
    let log = log();
    let mut kernel = fit_kernel(&log, SCREEN_REVISION, false, FIT);
    kernel.resize(Instant(100), 120, 60);
    assert_eq!(
        *log.borrow(),
        ["resize 120x60", "close", "drop", "made 120x60", "pane"]
    );
    assert!(kernel.notice.is_none(), "what it asked for was not done");
}

/// A screen of revision 1 (built before `on_resize` and `pane`) is never
/// asked them: made anew as screens always were, and `call_since` answers
/// for it what such a screen does.
#[test]
fn items_newer_than_a_screens_revision_are_never_called() {
    let log = log();
    let mut kernel = fit_kernel(&log, 1, true, FIT);
    kernel.resize(Instant(100), 120, 60);
    assert_eq!(*log.borrow(), ["close", "drop", "made 120x60"]);
    log.borrow_mut().clear();
    let screen = kernel.stack[0].screen.as_mut();
    let pane = paperpaw_sdk::call_since(screen, 2, Pane::FULL, |screen| screen.pane());
    assert_eq!(pane, Pane::FULL);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    // Of revision 2, it is asked.
    let mut kernel = fit_kernel(&log, 2, true, FIT);
    let screen = kernel.stack[0].screen.as_mut();
    let pane = paperpaw_sdk::call_since(screen, 2, Pane::FULL, |screen| screen.pane());
    assert_eq!(pane, Pane::DETAIL);
    assert_eq!(*log.borrow(), ["pane"]);
}

/// A host is of the revision of the screen it holds: holding one of
/// revision 2 it is asked to lay itself out anew and passes that on;
/// holding one of revision 1 it is not, so neither is the one it holds.
#[test]
fn a_host_passes_on_only_what_the_screen_it_holds_has() {
    let log = log();
    let mut kernel = fit_kernel(&log, SCREEN_REVISION, true, HOST);
    assert_eq!(kernel.stack[0].screen.revision(), SCREEN_REVISION);
    kernel.resize(Instant(100), 120, 60);
    assert_eq!(*log.borrow(), ["resize 120x60"]);

    let mut kernel = fit_kernel(&log, 1, true, HOST);
    assert_eq!(kernel.stack[0].screen.revision(), 1);
    kernel.resize(Instant(100), 120, 60);
    assert!(
        !log.borrow().iter().any(|line| line.starts_with("resize")),
        "{:?}",
        log.borrow()
    );
    // The host made anew holds a new one of the new size.
    assert_eq!(log.borrow().last().map(String::as_str), Some("made 120x60"));
}

/// A held screen that does not lay itself out anew is made anew by its
/// host, and what it asked for meanwhile does not reach the kernel, as
/// with a screen of the kernel's own.
#[test]
fn a_host_passes_on_nothing_of_a_screen_it_makes_anew() {
    let log = log();
    let mut kernel = fit_kernel(&log, SCREEN_REVISION, false, HOST);
    kernel.resize(Instant(100), 120, 60);
    assert_eq!(
        *log.borrow(),
        ["resize 120x60", "close", "drop", "made 120x60"]
    );
    assert!(kernel.notice.is_none(), "what it asked for was not done");
}
