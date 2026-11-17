//! Screens fitted to another size: laid out anew where they can
//! (`Screen::on_resize`), else closed, dropped and made anew; items newer
//! than a screen's revision never called on it.

use super::*;
use crate::Items;
use paperpaw_sdk::Pane;
use paperpaw_sdk::SCREEN_REVISION;

const FIT: Id = Id::new_static("fit.screen");

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

impl crate::Layer for Fitting {
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

/// The fitting screen, in the home slot.
struct FitPlugin {
    log: Log,
    build: Build,
    home: Id,
}

fn fit_plugin(kept: FitPlugin) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let (log, build) = (kept.log.clone(), kept.build);
        r.add_layer(crate::LayerSpec::new(
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
        r.bind_slot(slots::HOME, kept.home.clone(), Priority::DEFAULT)
    }
}

/// What [`fit_plugin`] registers under.
fn fit_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("fit"), "Fit", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn fit_kernel(log: &Log, revision: u16, takes: bool, home: Id) -> Kernel {
    let build = Build { revision, takes };
    let plugin = FitPlugin {
        log: log.clone(),
        build,
        home,
    };
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            crate::tests::notice_plugin_manifest(),
            crate::tests::notice_plugin(NoticePlugin { log: super::log() }),
        )
        .items(fit_plugin_manifest(), fit_plugin(plugin))
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
    let screen = &mut kernel.stack[0].screen;
    let pane = crate::screen::call_since(screen, 2, Pane::FULL, |screen| screen.pane());
    assert_eq!(pane, Pane::FULL);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    // Of revision 2, it is asked.
    let mut kernel = fit_kernel(&log, 2, true, FIT);
    let screen = &mut kernel.stack[0].screen;
    let pane = crate::screen::call_since(screen, 2, Pane::FULL, |screen| screen.pane());
    assert_eq!(pane, Pane::DETAIL);
    assert_eq!(*log.borrow(), ["pane"]);
}
