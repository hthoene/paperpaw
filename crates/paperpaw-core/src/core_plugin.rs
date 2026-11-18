//! The kernel's own contributions, registered through the regular plugin
//! interface: standard actions, slots, settings, key bindings and the
//! placeholder screen.

mod catalog_job;
mod clean_up;
mod look_again;
mod look_more;
mod reading;
mod setting_tables;
mod widget_settings;

use crate::Place;
use crate::Staging;
use crate::kit::action::ActionHint;
use crate::kit::action::FixedAction;
use crate::kit::library;
use crate::kit::menus::MenuEntry;
use crate::kit::status::StatusItemSpec;
use crate::kit::status::StatusValue;
use crate::kit::time::{self, AUTO, CLOCK_12H, CLOCK_24H, TIME_ZONES};
use crate::kit::widgets::{self, Drawn, WidgetSpec};
use crate::native_host::book_data_steps::{follow_move, follow_removal};
use crate::{Ask, Asks};
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use catalog_job::{CatalogJob, make_library_folder, waits};
use clean_up::{ask_clean_up, register_clean_up};
use paperpaw_sdk::builtin::{
    actions, events, group_sections, groups, keymap_presets, menus, params, screens, settings,
    settings_page, slots, status_items,
};
use paperpaw_sdk::{
    Id, Manifest, Params, Priority, RegisterError, SettingExtra, SettingGroupSpec, SettingKind,
    SettingSpec, SettingValue, SlotSpec,
};
use setting_tables::{
    register_colors, register_device_settings, register_general_settings, register_look_settings,
};

pub(crate) const CORE: Id = Id::new_static("core");

/// Settings the menus list beside actions, by their ids.
const NIGHT_MODE: Id = Id::new_static(settings::NIGHT_MODE.static_str());
const ROTATION: Id = Id::new_static(settings::ROTATION.static_str());

/// What the kernel's own contributions are listed and registered under.
pub(crate) fn manifest() -> Manifest {
    Manifest::new(CORE, "PaperPaw kernel", paperpaw_sdk::package_version!())
        .description("Runs screens, input, settings and plugins.")
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// The kernel's own contributions; `device_name` is the default of the
/// device's name.
pub(crate) fn items(device_name: &str, r: &mut Staging) -> Result<(), RegisterError> {
    register_screen_actions(r)?;
    register_moving(r)?;
    register_device_actions(r)?;
    register_library(r)?;
    register_reading_apps(r)?;
    register_screens(r)?;
    register_menus(r)?;
    register_general_settings(r)?;
    r.add_setting(paperpaw_sdk::device_name::setting(String::from(
        device_name,
    )))?;
    register_look_settings(r)?;
    register_device_settings(r)?;
    register_spaces(r)?;
    register_colors(r)?;
    reading::register(r)?;
    marks::register(r)?;
    register_time_and_battery(r)?;
    register_keymaps_and_setup(r)?;
    r.add_presets(&POWER_MODES)?;
    // Text typed for a setting (`ui::setting_opens`), whichever screen asked.
    r.on(events::SETTING_TYPED, |k, p| {
        k.with_event(&events::SETTING_TYPED, p, |env, typed, out| {
            // Only once the reader is done typing, not at each key.
            if typed.typing {
                return;
            }
            let id = typed
                .context
                .get(params::ID)
                .and_then(|id| Id::parse(id).ok());
            // The id typed for, a placement's too (`widgets.countdown@2.title`):
            // its spec is the plain setting's.
            let Some(id) = id else {
                return;
            };
            // Asked without its spec: what was typed is not dropped for
            // lack of room to read one (#248); the change itself says so.
            if !env.settings().knows(id.as_str()) {
                return;
            }
            // A secret setting's text is kept with the secrets, never in
            // the settings (see `Kernel::apply_setting`).
            let text = typed.text.trim();
            out.push(Ask::SetSetting(
                id,
                paperpaw_sdk::SettingValue::Text(Cow::Owned(String::from(text))),
            ));
        });
    })?;
    crate::work::register_network(r)?;
    Ok(())
}

/// The power modes built in (ADR 0025): a level, how much longer
/// declared work may wait, and values worn over the reader's own (a
/// number after `<=` is a limit).
static POWER_MODES: [paperpaw_sdk::presets::Preset; 3] = [
    paperpaw_sdk::presets::Preset::fixed(
        "core.normal",
        "core.power-mode",
        "core.power.normal",
        "core.power.normal.help",
        "power.level = normal",
    ),
    paperpaw_sdk::presets::Preset::fixed(
        "core.saving",
        "core.power-mode",
        "core.power.saving",
        "core.power.saving.help",
        "power.level = saving\npower.stretch = 4\ncore.sleep-after <= 5\nx4.frontlight <= 50",
    ),
    paperpaw_sdk::presets::Preset::fixed(
        "core.offline",
        "core.power-mode",
        "core.power.offline",
        "core.power.offline.help",
        "power.level = offline\npower.stretch = 4\ncore.sleep-after <= 5\nx4.frontlight <= 30",
    ),
];

/// The reading app chosen from the list: for a book (`b:` and its path),
/// a folder (`f:`), or to open a book this time (`o:`).
const READING_APP_PICKED: paperpaw_sdk::event::Event<crate::kit::ui::choice::Picked> =
    paperpaw_sdk::event::Event::new("core.reading-app-picked");

/// In the list to open a book this time: asks for the reading app kept
/// for it.
const ALWAYS: &str = "core.reading-app.always";

/// Choosing the reading app of a book or a folder (see
/// `crate::kit::readers::ask`): the readers that fit are listed; the
/// one chosen is kept, and a book opens with it; *Standard* forgets the
/// choice. *Open with…* (the book menu's, [`register_open_with`], or the
/// file manager's) lists the readers of the book, the other ways to open it
/// plugins add (`menus::OPEN_WITH`) and *Always open it with…*: the one
/// chosen opens it this time.
fn register_reading_apps(r: &mut Staging) -> Result<(), RegisterError> {
    use crate::kit::readers::{self, Choice};
    #[cfg(not(feature = "native-reading"))]
    register_open_with(r)?;
    r.on(events::READING_APP_WANTED, |k, p| {
        k.with_event(&events::READING_APP_WANTED, p, |env, wanted, out| {
            let path = wanted.path.trim_matches('/');
            let name = paperpaw_sdk::files::name(path);
            let once = wanted.once == Some(true) && !wanted.folder;
            let (options, key, kind) = if once {
                let mut options: Vec<&str> = env
                    .menus()
                    .items(menus::OPEN_WITH.as_str())
                    .into_iter()
                    .filter(|item| !item.section)
                    .map(|item| item.id.as_str())
                    .collect();
                options.push(ALWAYS);
                (options, "core.reading-app.open-with", "o")
            } else if wanted.folder {
                let options = alloc::vec![settings::READING_APP_DEFAULT];
                (options, "core.reading-app.for-folder", "f")
            } else {
                let options = alloc::vec![settings::READING_APP_DEFAULT];
                (options, "core.reading-app.always-with", "b")
            };
            let title = env
                .i18n()
                .format(key, &[("name", paperpaw_sdk::Arg::Str(name))]);
            out.push(
                crate::kit::ask::choice(title, &READING_APP_PICKED)
                    .about(alloc::format!("{kind}:{path}"))
                    .options("", &options)
                    .command(),
            );
        });
    })?;
    r.on(READING_APP_PICKED, |k, p| {
        k.with_event(&READING_APP_PICKED, p, |env, picked, out| {
            let Some((kind, path)) = picked.path.split_once(':') else {
                return;
            };
            let option = picked.option.as_str();
            if kind == "o" {
                if option == ALWAYS {
                    out.push(readers::ask(Choice::Book(path)));
                } else if let Ok(action) = Id::parse(option) {
                    // Another way to open it, as an action of the book menu.
                    let chosen = events::BookChosen::new(action.clone(), String::from(path));
                    out.push(crate::Ask::emit(&events::BOOK_CHOSEN, chosen));
                    out.push(Ask::run(action));
                }
                return;
            }
            let choice = match kind {
                "f" => Choice::Folder(path),
                _ => Choice::Book(path),
            };
            if readers::remember(env.storage(), choice, None).is_err() {
                let text = env.i18n().tr("core.reading-app.not-kept");
                out.push(Ask::Notify(String::from(text)));
            }
            if kind == "b" {
                out.push(readers::open(path));
            }
        });
    })
}

/// *Open with…* in the book menu (`readers::ask_once`), where a book has
/// another reader or another way to open it. Not with the linked native
/// reader (`native-reading`): the one-time choice there is the file
/// manager's, and this one's resolver is interface 6's.
#[cfg(not(feature = "native-reading"))]
fn register_open_with(r: &mut Staging) -> Result<(), RegisterError> {
    use crate::kit::readers;
    // What it does is asked once the book is known (`BOOK_CHOSEN` below).
    static OPEN_WITH: [FixedAction; 1] =
        [FixedAction::new(actions::OPEN_WITH, "core.action.open-with").handler(|_, _, _| {})];
    static OPEN_WITH_ENTRY: [MenuEntry; 1] = [MenuEntry::new(menus::BOOK, actions::OPEN_WITH)];
    static OPEN_WITH_ICON: [(Id, &str); 1] = [(actions::OPEN_WITH, "apps")];
    r.add_actions(&OPEN_WITH)?;
    r.add_menu_entries(&OPEN_WITH_ENTRY)?;
    r.add_icons(&OPEN_WITH_ICON)?;
    // Where there is something to choose: another reading app, or another
    // way to open books.
    r.set_item_state(actions::OPEN_WITH, |env| {
        let others = !env.menus().items(menus::OPEN_WITH.as_str()).is_empty();
        crate::kit::menus::ItemState::new().shown(others)
    })?;
    r.on(events::BOOK_CHOSEN, |k, p| {
        k.with_event(&events::BOOK_CHOSEN, p, |_, chosen, out| {
            if chosen.action == actions::OPEN_WITH {
                out.push(readers::ask_once(&chosen.path));
            }
        });
    })?;
    Ok(())
}

/// The actions screens give a meaning of their own (with their hints in
/// the row), and how a screen takes gestures.
fn register_screen_actions(r: &mut Staging) -> Result<(), RegisterError> {
    static SCREEN_ACTIONS: [FixedAction; 10] = [
        FixedAction::new(actions::NEXT_PAGE, "core.action.next-page")
            .hint(ActionHint::Icon("chevron-down")),
        FixedAction::new(actions::PREV_PAGE, "core.action.prev-page")
            .hint(ActionHint::Icon("chevron-up")),
        FixedAction::new(actions::UP, "core.action.up").hint(ActionHint::Icon("chevron-up")),
        FixedAction::new(actions::DOWN, "core.action.down").hint(ActionHint::Icon("chevron-down")),
        FixedAction::new(actions::LEFT, "core.action.left").hint(ActionHint::Icon("chevron-left")),
        FixedAction::new(actions::RIGHT, "core.action.right")
            .hint(ActionHint::Icon("chevron-right")),
        FixedAction::new(actions::CONFIRM, "core.action.confirm")
            .hint(label("core.hint.core.confirm")),
        // How a screen takes gestures (`gestures.policy` in key maps):
        // ids to bind, nothing to run.
        FixedAction::new(
            Id::new_static("core.gestures.standard"),
            "core.gestures.standard",
        ),
        FixedAction::new(Id::new_static("core.gestures.own"), "core.gestures.own"),
        FixedAction::new(Id::new_static("core.gestures.off"), "core.gestures.off"),
    ];
    r.add_actions(&SCREEN_ACTIONS)
}

/// The actions screens are moved and left with, and what opens the
/// kernel's slots and menus.
fn register_moving(r: &mut Staging) -> Result<(), RegisterError> {
    static MOVING: [FixedAction; 11] = [
        FixedAction::new(actions::REFRESH_SCREEN, "core.action.refresh-screen").handler(
            |_, _, out| {
                out.push(Ask::Redraw(paperpaw_sdk::RefreshMode::Full));
            },
        ),
        FixedAction::new(actions::QUICK_PANEL, "core.action.quick-panel").handler(|_, _, out| {
            out.push(Ask::OpenSlot(
                slots::PANEL,
                Params::new()
                    .with(params::MENU, String::from(menus::QUICK.as_str()))
                    .with(params::MODE, "top"),
            ));
        }),
        // Screens without their own meaning of next/previous turn pages.
        FixedAction::new(actions::NEXT, "core.action.next")
            .handler(|_, _, out| out.push(Ask::run(actions::NEXT_PAGE))),
        FixedAction::new(actions::PREVIOUS, "core.action.previous")
            .handler(|_, _, out| out.push(Ask::run(actions::PREV_PAGE))),
        FixedAction::new(actions::MENU, "core.action.menu")
            .hint(label("core.hint.core.menu"))
            .handler(|_, _, out| {
                out.push(Ask::OpenSlot(
                    slots::MENU,
                    Params::new().with(params::MENU, menus::MAIN.as_str()),
                ));
            }),
        // Select held: the menu, where no screen takes it for a long
        // press of its own.
        FixedAction::new(actions::LONG_CONFIRM, "core.action.long-confirm")
            .handler(|_, _, out| out.push(Ask::run(actions::MENU))),
        FixedAction::new(actions::LIBRARY, "core.action.library")
            .handler(|_, _, out| out.push(Ask::open_slot(slots::LIBRARY))),
        FixedAction::new(actions::SETTINGS, "core.action.settings")
            .handler(|_, _, out| out.push(Ask::open_slot(slots::SETTINGS))),
        FixedAction::new(actions::BACK, "core.action.back")
            .hint(label("core.hint.core.back"))
            .handler(|_, _, out| out.push(Ask::Back)),
        FixedAction::new(actions::NOTHING, "core.action.nothing")
            .hint(ActionHint::Hidden)
            .handler(|_, _, _| {}),
        FixedAction::new(actions::HOME, "core.action.home")
            .hint(label("core.hint.core.home"))
            .handler(|_, _, out| out.push(Ask::open_slot(slots::HOME))),
    ];
    r.add_actions(&MOVING)
}

/// The actions of the device: cleaning up, screenshots, the tools,
/// sleep and night mode.
fn register_device_actions(r: &mut Staging) -> Result<(), RegisterError> {
    static DEVICE: [FixedAction; 6] = [
        FixedAction::new(actions::CLEAN_UP, "core.action.clean-up")
            .handler(|env, _, out| ask_clean_up(env, out)),
        FixedAction::new(actions::SCREENSHOT, "core.action.screenshot").handler(|env, _, out| {
            out.push(Ask::Screenshot(screenshot_path(env)));
        }),
        FixedAction::new(actions::TOOLS, "core.action.tools").handler(|_, _, out| {
            out.push(Ask::OpenSlot(
                slots::MENU,
                Params::new().with(params::MENU, menus::TOOLS.as_str()),
            ));
        }),
        FixedAction::new(actions::SLEEP, "core.action.sleep").handler(|env, _, out| {
            // The sleep screen first, if there is one, then sleep.
            if env.caps().sleep && env.has_slot(&slots::SLEEP) {
                out.push(Ask::open_slot(slots::SLEEP));
            }
            out.push(Ask::Sleep);
        }),
        FixedAction::new(USB_DRIVE, "core.action.usb-drive").handler(|env, _, out| {
            // The notice first: the frame shown says it before the card goes
            // to the computer (`Command::UsbDrive`).
            out.push(
                crate::kit::ask::confirm(env.i18n().tr("core.usb-drive.title"), &USB_DRIVE_NOTICE)
                    .text(env.i18n().tr("core.usb-drive.text"))
                    .only_yes()
                    .command(),
            );
            out.push(Ask::UsbDrive);
        }),
        FixedAction::new(actions::TOGGLE_NIGHT_MODE, "core.action.toggle-night-mode").handler(
            |env, _, out| {
                let night = env.settings().read(&settings::NIGHT_MODE);
                out.push(Ask::set(&settings::NIGHT_MODE, !night));
            },
        ),
    ];
    r.add_actions(&DEVICE)?;
    // Only where the device can be a drive (the S3 boards).
    static ENTRY: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, USB_DRIVE).order(80)];
    r.add_menu_entries(&ENTRY)?;
    r.set_menu_need(
        USB_DRIVE,
        paperpaw_sdk::MenuNeed::Has(paperpaw_sdk::Capability::USB_DRIVE),
    )
}

/// *USB drive* in the tools: the card shown to a computer
/// ([`Command::UsbDrive`]).
const USB_DRIVE: Id = Id::new_static("core.usb-drive");

/// The notice shown while the computer has the card; nobody answers it.
const USB_DRIVE_NOTICE: paperpaw_sdk::event::Event<paperpaw_sdk::replies::Confirmed> =
    paperpaw_sdk::event::Event::new("core.usb-drive-notice");

/// The work that makes the library's catalog anew or catches it up
/// ([`library::catalog`]).
pub(crate) const CATALOG_WORK: Id = Id::new_static("core.catalog");

/// The making of the catalog, shared by the kernel's handlers and its
/// job: the only writer of the catalog, one change at a time.
#[derive(Default)]
struct Catalog {
    job: core::cell::RefCell<Option<CatalogJob>>,
    /// A walk is wanted (else catching up, for `fresh` or a long log).
    walk: core::cell::Cell<bool>,
    /// A walk was wanted anew while one is under way: it starts again.
    restart: core::cell::Cell<bool>,
    /// Books to take from their details: they came, moved here, changed.
    fresh: core::cell::RefCell<Vec<String>>,
    /// What the job under way took of `fresh`, given back where it fails.
    taken: core::cell::RefCell<Vec<String>>,
    /// Wanted at once.
    urgent: core::cell::Cell<bool>,
    /// The library was walked since PaperPaw started.
    walked: core::cell::Cell<bool>,
    /// The books opened last were caught up before the first walk since
    /// PaperPaw started ([`library::catalog::CatchUp`]).
    caught: core::cell::Cell<bool>,
    /// What earlier versions kept for books is to be moved
    /// ([`library::split`]): first, before a walk or catching up.
    split: core::cell::Cell<bool>,
    /// The archive folders the views of the sorted books leave out
    /// ([`library::sorted`]), those asked for last first.
    archives: core::cell::RefCell<Vec<String>>,
}

impl Catalog {
    /// Wants the library walked: `now`, or at a calm moment.
    /// Wants the library walked: `now`, or at a calm moment; `anew`
    /// where what a walk under way met may have changed (it starts again),
    /// else one under way, paused or not, goes on.
    fn want_walk(&self, now: bool, anew: bool, out: &mut Asks) {
        let walking = self.job.borrow().as_ref().is_some_and(CatalogJob::is_walk);
        if walking && !anew {
            self.declare(now, out);
            return;
        }
        self.restart.set(self.job.borrow().is_some());
        self.walk.set(true);
        self.declare(now, out);
    }

    /// Wants the catalog caught up, with the book at `path` taken from its
    /// details: `now`, or at a calm moment (books coming one after the
    /// other, from Calibre say, caught up once).
    fn want_catch_up(&self, path: Option<&str>, now: bool, out: &mut Asks) {
        if let Some(path) = path {
            let mut fresh = self.fresh.borrow_mut();
            if !fresh.iter().any(|p| p == path) {
                fresh.push(path.into());
            }
        }
        self.declare(now, out);
    }

    /// Declares the work: at once, or at a calm moment (no input for a
    /// while, a book closed, the device plugged in; within a minute at the
    /// latest), never while the device goes to sleep or opens a book. A
    /// calm request does not put off one at once.
    fn declare(&self, now: bool, out: &mut Asks) {
        use paperpaw_sdk::work::{Moments, Work};
        self.urgent.set(self.urgent.get() || now);
        let calm = Moments::IDLE | Moments::BOOK_CLOSED | Moments::CHARGING;
        let within = if self.urgent.get() { 0 } else { 60 };
        out.push(Ask::work(
            Work::new(CATALOG_WORK)
                .at_moments(calm)
                .within(core::time::Duration::from_secs(within)),
        ));
    }

    /// The archive of a view the sorted books are to be made for, where
    /// they are not of the library there is now.
    fn sort_wanted(&self, env: &Place<'_>) -> Option<String> {
        let archives = self.archives.borrow();
        let mut wanted = archives.iter().filter(|a| library::sorted::wanted(env, a));
        wanted.next().cloned()
    }

    /// Gives what the job under way took back, for the next.
    fn give_back(&self, walk: bool) {
        let taken = core::mem::take(&mut *self.taken.borrow_mut());
        let mut fresh = self.fresh.borrow_mut();
        for path in taken {
            if !fresh.contains(&path) {
                fresh.push(path);
            }
        }
        self.walk.set(self.walk.get() || walk);
    }

    /// A step of moving what earlier versions kept for books: then on to
    /// a walk or catching up, if wanted. Failed, it is tried at the next
    /// start, not before each walk (an old folder that cannot be read would
    /// keep the catalog from being made); the books are read where they were.
    fn split_step(
        &self,
        env: &Place<'_>,
        job: &mut Option<CatalogJob>,
    ) -> paperpaw_sdk::work::Step {
        use paperpaw_sdk::work::Step;
        match job.as_mut().map_or(Ok(true), |job| job.step(env)) {
            Ok(false) => Step::Continue,
            Ok(true) => {
                *job = None;
                if self.walk.get() || !self.fresh.borrow().is_empty() {
                    Step::Continue
                } else {
                    self.urgent.set(false);
                    Step::Done
                }
            }
            Err(error) => {
                *job = None;
                Step::Failed(alloc::format!("library folders: {error:?}"))
            }
        }
    }

    /// One step of the job.
    fn step(&self, env: &Place<'_>, out: &mut Asks) -> paperpaw_sdk::work::Step {
        use paperpaw_sdk::work::Step;
        let storage = env.storage();
        let mut job = self.job.borrow_mut();
        if waits(env) {
            // Goes on where it was once another screen shows.
            if let Some(running) = job.as_mut() {
                running.pause(storage);
            }
            return Step::WaitFor(events::SCREEN_SHOWN.id().clone());
        }
        if self.restart.replace(false) {
            self.stop(&mut job, storage);
        }
        if job.is_none() && self.split.replace(false) {
            *job = Some(CatalogJob::Split(library::split::Split::new()));
        }
        if matches!(*job, Some(CatalogJob::Split(_))) {
            return self.split_step(env, &mut job);
        }
        if job.is_none()
            && let Some(ended) = self.start(env, &mut job, out)
        {
            return ended;
        }
        let walk = job.as_ref().is_some_and(CatalogJob::is_walk);
        let result = job.as_mut().map_or(Ok(true), |job| job.step(env));
        match result {
            Ok(false) => catalog_job::after_step(self, env, &mut job, out),
            Ok(true) => {
                // Books appended: views hear of them at once (the sorted
                // books are made anew after the walk); nothing appended,
                // nothing changed.
                let caught = match &*job {
                    Some(CatalogJob::CatchUp(catch_up)) => Some(catch_up.appended()),
                    _ => None,
                };
                *job = None;
                self.taken.borrow_mut().clear();
                self.walked.set(self.walked.get() || walk);
                // Views hear of a catalog made anew once its books are
                // sorted for each (a file made is wanted no more).
                let sorting = self.sort_wanted(env).is_some();
                if caught.unwrap_or(!sorting) {
                    out.push(crate::Ask::emit(&events::CATALOG_CHANGED, ()));
                }
                if sorting || self.walk.get() || !self.fresh.borrow().is_empty() {
                    Step::Continue
                } else {
                    self.urgent.set(false);
                    Step::Done
                }
            }
            Err(error) => {
                // The old catalog stays; tried again after a while. Views
                // stop saying the books are being collected.
                *job = None;
                self.give_back(walk);
                out.push(crate::Ask::emit(&events::CATALOG_FAILED, ()));
                Step::Failed(alloc::format!("catalog: {error:?}"))
            }
        }
    }
}

/// The library service and what plugins keep for books, which follow
/// the books.
fn register_library(r: &mut Staging) -> Result<(), RegisterError> {
    // The catalog, made anew or caught up as a job: a few books a step,
    // after the frame and while no input waits; views hear
    // CATALOG_CHANGED.
    r.on(events::STARTED, |k, p| {
        k.with_event(&events::STARTED, p, |env, _, _| make_library_folder(env));
    })?;
    // What earlier versions kept for books in one folder each: moved at a
    // calm moment after the start, a few at a time.
    r.on(events::STARTED, |k, p| {
        k.with_core_event(&events::STARTED, p, |env, core, _, out| {
            // What a making of the sorted books cut short left goes; the
            // views they were for are those they are made anew for.
            library::sorted::clean_up(env.storage());
            *core.catalog.archives.borrow_mut() = library::sorted::kept_archives(env.storage());
            if library::split::wanted(env.storage()) {
                core.catalog.split.set(true);
                core.catalog.declare(false, out);
            }
        });
    })?;

    r.add_work(crate::work::WorkSpec::new(
        CATALOG_WORK,
        "core.work.catalog",
        |k, _| {
            let mut step = paperpaw_sdk::work::Step::Done;
            k.with_core(|env, core, out| step = core.catalog.step(env, out));
            step
        },
    ))?;
    // Walked when a view of the library first shows after a start or a
    // wake (books may have been copied meanwhile) or asks for it anew, for
    // another library folder, and when books came or went without saying
    // which (`LIBRARY_CHANGED`): at once, taking the books whose files did
    // not change from the old catalog. Never at every start or wake by
    // itself: on the X4 they are many. Without a format nothing is a book.
    r.on(events::CATALOG_WANTED, |k, p| {
        k.with_core_event(&events::CATALOG_WANTED, p, |env, core, wanted, out| {
            let missing =
                library::catalog::state(env.storage()) == library::catalog::State::Missing;
            if !env.formats().is_empty() && (wanted.anew || missing || !core.catalog.walked.get()) {
                core.catalog.want_walk(true, wanted.anew, out);
            } else if library::catalog::log_is_long(env.storage()) {
                // The library service's status notes made the log long: it
                // is folded in at a calm moment, as after a book closed.
                core.catalog.want_catch_up(None, false, out);
            } else if library::emptiness::is_empty(env.storage()) {
                // Walked since the start and listing nothing: not again
                // (a walk costs a wake's time and battery on the X4), but
                // the view that asked is told, so that it says there are
                // no books rather than that they are being collected.
                out.push(crate::Ask::emit(&events::CATALOG_CHANGED, ()));
            }
        });
    })?;

    catalog_job::register_sorted(r)?;
    look_again::register(r)?;
    r.on(events::LIBRARY_CHANGED, |k, p| {
        k.with_core_event(&events::LIBRARY_CHANGED, p, |_, core, _, out| {
            core.catalog.want_walk(true, true, out);
        });
    })?;

    r.on(events::SETTING_CHANGED, |k, p| {
        k.with_core_event(&events::SETTING_CHANGED, p, |_, core, changed, out| {
            if changed.id == *library::FOLDER.id() {
                core.catalog.want_walk(true, true, out);
            }
        });
    })?;

    // A book came, or changed what the catalog keeps of it: caught up at
    // a calm moment, without walking the library.
    r.on(events::BOOK_ADDED, |k, p| {
        k.with_core_event(&events::BOOK_ADDED, p, |_, core, added, out| {
            core.catalog.want_catch_up(Some(&added.path), false, out);
        });
    })?;

    r.on(events::BOOK_CHANGED, |k, p| {
        k.with_core_event(&events::BOOK_CHANGED, p, |_, core, _, _| {
            core.recorded.take();
        });
    })?;

    register_reading(r)?;
    // What plugins keep for a book follows it: moved, removed. The catalog
    // leaves it out at once (noted beside it); a book moved is caught up
    // where it is now, at once (the book is not where views show it).
    r.on(events::BOOK_MOVED, |k, p| {
        k.with_core_event(&events::BOOK_MOVED, p, |env, core, moved, out| {
            core.recorded.take();
            library::moved(env.storage(), &moved.from, &moved.path);
            follow_move(env.storage(), env.book_data(), &moved.from, &moved.path);
            let _ = library::catalog::forget(env.storage(), &moved.from);
            core.catalog.fresh.borrow_mut().retain(|p| *p != moved.from);
            core.catalog.want_catch_up(Some(&moved.path), true, out);
        });
    })?;

    r.on(events::BOOK_REMOVED, |k, p| {
        k.with_core_event(&events::BOOK_REMOVED, p, |env, core, removed, out| {
            core.recorded.take();
            library::removed(env.storage(), &removed.path);
            follow_removal(env.storage(), env.book_data(), &removed.path);
            let _ = library::catalog::forget(env.storage(), &removed.path);
            core.catalog
                .fresh
                .borrow_mut()
                .retain(|p| *p != removed.path);
            if library::catalog::log_is_long(env.storage()) {
                core.catalog.want_catch_up(None, false, out);
            }
        });
    })?;

    register_clean_up(r)
}

/// What the core plugin's handlers keep between calls, the kernel's
/// ([`Kernel::with_core`]).
#[derive(Default)]
pub(crate) struct State {
    /// The catalog's job and what it is to do.
    catalog: Catalog,
    /// The book and the percentage recorded at the last page turned: the
    /// details are written only when it changes, so a page turned that
    /// does not change it reads nothing. Forgotten when the book opens,
    /// closes or changes.
    recorded: core::cell::RefCell<Option<(String, u8)>>,
}

fn register_reading(r: &mut Staging) -> Result<(), RegisterError> {
    // The library service follows the reading, whichever plugins
    // show it: how far a book is read, in its details and, when it is
    // opened, closed or finished, beside the catalog (folded into it at a
    // calm moment once the log is long).
    r.on(events::BOOK_OPENED, |k, p| {
        k.with_core_event(&events::BOOK_OPENED, p, |env, core, reading, out| {
            core.recorded.take();
            library::record(env, reading, true);
            out.push(library::changed_book(
                &reading.path,
                library::what::PROGRESS,
            ));
        });
    })?;

    r.on(events::PAGE_TURNED, |k, p| {
        k.with_core_event(&events::PAGE_TURNED, p, |env, core, reading, _| {
            let percent = (reading.progress + 0.5).clamp(0.0, 100.0) as u8;
            let mut recorded = core.recorded.borrow_mut();
            if recorded
                .as_ref()
                .is_some_and(|(path, p)| *p == percent && *path == reading.path)
            {
                return;
            }
            library::record(env, reading, false);
            *recorded = Some((reading.path.clone(), percent));
        });
    })?;

    for event in [events::BOOK_CLOSED, events::BOOK_FINISHED] {
        r.on(event, |k, p| {
            k.with_core_event(&events::BOOK_CLOSED, p, |env, core, reading, out| {
                core.recorded.take();
                library::record(env, reading, false);
                library::note_status(env.storage(), &reading.path);
                out.push(library::changed_book(
                    &reading.path,
                    library::what::PROGRESS,
                ));
                if library::catalog::log_is_long(env.storage()) {
                    core.catalog.want_catch_up(None, false, out);
                }
            });
        })?;
    }
    Ok(())
}

/// The work that cleans up the storage by itself.
const CLEAN_UP_WORK: Id = Id::new_static("core.clean-up-work");
/// When the storage was cleaned up last, in seconds since 1970.
const CLEANED_UP: &str = paperpaw_sdk::data_path!("cleaned-up.txt");
/// How often the storage is cleaned up by itself, at most.
const CLEAN_UP_EVERY: i64 = 7 * 24 * 3600;
/// How long after the first start it is cleaned up by itself first.
const CLEAN_UP_FIRST: i64 = 24 * 3600;

/// The answer whether the reader's data of books not found goes.
const CLEAN_UP_ANSWERED: paperpaw_sdk::event::Event<paperpaw_sdk::replies::Confirmed> =
    paperpaw_sdk::event::Event::new("core.clean-up-answered");

/// Declares cleaning up by itself: while charging, a week after it was
/// last done (a day after the first start). Not without a clock, which
/// tells the time across starts (on the X4 every wake is one).
fn declare_clean_up(env: &crate::Place<'_>, out: &mut Asks) {
    use paperpaw_sdk::work::{Moments, Work};
    let Some(now) = crate::kit::time::unix_seconds(env) else {
        return;
    };
    let last = paperpaw_sdk::read_file(env.storage(), CLEANED_UP, 32)
        .ok()
        .and_then(|bytes| {
            core::str::from_utf8(&bytes)
                .ok()?
                .trim()
                .parse::<i64>()
                .ok()
        });
    let last = last.unwrap_or_else(|| {
        // Seen first now: as if done the right while ago.
        let seen = now - (CLEAN_UP_EVERY - CLEAN_UP_FIRST);
        let _ = env
            .storage()
            .write(CLEANED_UP, alloc::format!("{seen}\n").as_bytes());
        seen
    });
    let wait = (last + CLEAN_UP_EVERY - now).clamp(0, CLEAN_UP_EVERY);
    out.push(Ask::work(
        Work::new(CLEAN_UP_WORK)
            .charging()
            .after(core::time::Duration::from_secs(wait as u64))
            .at_moments(Moments::CHARGING | Moments::IDLE),
    ));
}

/// The kernel's slots, with a placeholder under each, and its own
/// screens.
fn register_screens(r: &mut Staging) -> Result<(), RegisterError> {
    for (id, title) in [
        (slots::HOME, "core.slot.home"),
        (slots::READER, "core.slot.reader"),
        (slots::LIBRARY, "core.slot.library"),
        (slots::MENU, "core.slot.menu"),
        (slots::SETTINGS, "core.slot.settings"),
    ] {
        r.add_slot(SlotSpec::new(id.clone(), title))?;
        // Lowest possible priority: any real screen wins.
        r.bind_slot(id, screens::PLACEHOLDER, Priority(i16::MIN))?;
    }
    // The choice and the yes or no question are the dialog plugin's
    // (`core.choice`, `core.confirm`), a list of widgets the sleep plugin's
    // (`core.edit-widgets`), a folder for a setting the folder chooser's,
    // whichever plugin fills those slots (#293).
    r.add_host_screens(&crate::host_screen::ENTRIES)?;
    // Notices are the host's (`docs/abi/ui.md`, *Messages*) where no plugin
    // shows them its own way (#257).
    r.bind_slot(
        slots::NOTICE,
        crate::host_screen::NOTICE,
        Priority::FALLBACK,
    )?;
    Ok(())
}

/// The main menu, the tools, the quick panel and the settings' sections
/// and groups.
fn register_menus(r: &mut Staging) -> Result<(), RegisterError> {
    // Every reader's submenu "More" (`menus::READER_MORE`): a submenu is
    // an action named like its menu, which the menu shows as a submenu
    // and which opens the menu when bound to a key. The same for what
    // there is to open, a submenu of the tools (`menus::APPS`) where
    // plugins put something there: reached with the buttons too, however
    // full the home screen is.
    static SUBMENUS: [FixedAction; 2] = [
        FixedAction::new(menus::READER_MORE, "core.action.reader-more").handler(|_, _, out| {
            out.push(Ask::OpenSlot(
                slots::MENU,
                Params::new().with(params::MENU, menus::READER_MORE.as_str()),
            ));
        }),
        FixedAction::new(menus::APPS, "core.action.apps").handler(|_, _, out| {
            out.push(Ask::OpenSlot(
                slots::MENU,
                Params::new().with(params::MENU, menus::APPS.as_str()),
            ));
        }),
    ];
    r.add_actions(&SUBMENUS)?;
    r.set_item_state(menus::APPS, |env| {
        crate::kit::menus::ItemState::new()
            .shown(!env.menus().items(menus::APPS.as_str()).is_empty())
    })?;
    // A short main menu; the tools plugins bring are one entry, so it
    // stays the same however many there are. The quick panel holds what
    // is changed often, whatever is open (the quick panel at the top of
    // the main menu: found without its gesture or key); plugins add
    // their own (the light, ...).
    static ENTRIES: [MenuEntry; 14] = [
        MenuEntry::new(menus::TOOLS, actions::SCREENSHOT).order(95),
        MenuEntry::new(menus::TOOLS, menus::APPS).order(90),
        MenuEntry::new(menus::MAIN, actions::QUICK_PANEL).order(5),
        MenuEntry::new(menus::MAIN, actions::HOME).order(10),
        MenuEntry::new(menus::MAIN, actions::LIBRARY).order(20),
        MenuEntry::new(menus::MAIN, actions::TOOLS).order(30),
        MenuEntry::new(menus::MAIN, NIGHT_MODE).order(85),
        MenuEntry::new(menus::MAIN, actions::SETTINGS).order(90),
        MenuEntry::new(menus::QUICK, NIGHT_MODE).order(10),
        MenuEntry::new(menus::QUICK, ROTATION).order(40),
        MenuEntry::new(menus::QUICK, actions::REFRESH_SCREEN).order(50),
        MenuEntry::new(menus::QUICK, actions::SCREENSHOT).order(60),
        MenuEntry::new(menus::QUICK, actions::SLEEP).order(70),
        MenuEntry::new(menus::QUICK, actions::SETTINGS).order(90),
    ];
    r.add_menu_entries(&ENTRIES)?;
    static ICONS: [(Id, &str); 21] = [
        (menus::READER_MORE, "dots"),
        (actions::SCREENSHOT, "camera"),
        (menus::APPS, "apps"),
        (actions::QUICK_PANEL, "adjustments-horizontal"),
        (actions::HOME, "home"),
        (actions::LIBRARY, "books"),
        (actions::TOOLS, "apps"),
        (NIGHT_MODE, "moon"),
        (actions::SETTINGS, "settings"),
        (actions::REFRESH_SCREEN, "sparkles"),
        (groups::READING, "book"),
        (groups::LIBRARY, "books"),
        (groups::DISPLAY, "contrast"),
        (groups::CONTROLS, "adjustments"),
        (groups::SYSTEM, "device-tablet"),
        (groups::LOOK, "palette"),
        (groups::OTHER, "dots"),
        (groups::DEVELOPER, "bug"),
        (groups::TIME, "clock"),
        (groups::PLUGINS, "puzzle"),
        (groups::WORDS, "language"),
    ];
    r.add_icons(&ICONS)?;

    // The settings' main page: reading, the device, customising.
    for (id, title, order) in [
        (settings_page::READING, "core.settings-section.reading", 0),
        (settings_page::DEVICE, "core.settings-section.device", 50),
        (
            settings_page::CUSTOMISE,
            "core.settings-section.customise",
            200,
        ),
    ] {
        r.add_menu_section(
            crate::kit::menus::MenuSection::new(id, title, menus::SETTINGS).order(order),
        )?;
    }
    static GROUPS: [SettingGroupSpec; 11] = [
        SettingGroupSpec::new(groups::READING, "core.group.reading").order(10),
        SettingGroupSpec::new(groups::LIBRARY, "core.group.library").order(20),
        SettingGroupSpec::new(groups::DISPLAY, "core.group.display").order(60),
        SettingGroupSpec::new(groups::CONTROLS, "core.group.controls").order(70),
        SettingGroupSpec::new(groups::SYSTEM, "core.group.system").order(90),
        SettingGroupSpec::new(groups::LOOK, "core.group.look").order(210),
        SettingGroupSpec::new(groups::OTHER, "core.group.other").order(100),
        SettingGroupSpec::new(groups::DEVELOPER, "core.group.developer").order(95),
        SettingGroupSpec::new(groups::TIME, "core.group.time").order(89),
        SettingGroupSpec::new(groups::PLUGINS, "core.group.plugins").order(230),
        SettingGroupSpec::new(groups::WORDS, "core.group.words").order(12),
    ];
    for group in &GROUPS {
        r.add_setting_group(group)?;
    }
    Ok(())
}

/// Units, steps and sections of the kernel's settings, and the numbers
/// set by eye, with sliders.
static MM: SettingExtra = SettingExtra::new().unit("core.unit.mm");
static MM_SLIDER: SettingExtra = SettingExtra::new().unit("core.unit.mm").slider();
static PT_SLIDER: SettingExtra = SettingExtra::new().unit("core.unit.pt").slider();
static PAGES: SettingExtra = SettingExtra::new().unit("core.unit.pages");
static SECONDS: SettingExtra = SettingExtra::new().unit("core.unit.seconds");
static MS: SettingExtra = SettingExtra::new().unit("core.unit.ms");
static TENTH_MM: SettingExtra = SettingExtra::new().unit("core.unit.tenth-mm");
static PERCENT_10_SLIDER: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .step(10)
    .slider();
static PERCENT_5_SLIDER: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .step(5)
    .slider();
static PERCENT_5: SettingExtra = SettingExtra::new().unit("core.unit.percent").step(5);
static MINUTES_5: SettingExtra = SettingExtra::new().unit("core.unit.minutes").step(5);
static COLORS: SettingExtra = SettingExtra::new().section("core.section.colors");
static FONT: SettingExtra = SettingExtra::new().section(group_sections::FONT);
static FONT_PT: SettingExtra = SettingExtra::new()
    .section(group_sections::FONT)
    .unit("core.unit.pt")
    .slider();
static WEIGHT: SettingExtra = SettingExtra::new()
    .unit("core.unit.weight")
    .section(group_sections::FONT);
static WORDS: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .step(10)
    .section(group_sections::FONT)
    .slider();
static LINES: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .step(5)
    .section(group_sections::PAGE)
    .slider();
static MARGINS: SettingExtra = SettingExtra::new()
    .unit("core.unit.mm")
    .section(group_sections::PAGE)
    .slider();
static MARGINS_ENDS: SettingExtra = SettingExtra::new()
    .unit("core.unit.mm")
    .section(group_sections::PAGE)
    .slider()
    .follows(Id::new_static(settings::MARGIN.static_str()));
static IN_PAGE: SettingExtra = SettingExtra::new().section(group_sections::PAGE);
/// The reading apps' part of the reading settings: the default one and
/// one for each format (registered once the formats are known).
/// A reading app chosen whose plugin is gone is kept, and passed over.
static IN_APPS: SettingExtra = SettingExtra::new()
    .section("core.section.reading-apps")
    .keeps_unknown();

mod marks;
/// Contrast, gamma and dither of pictures.
pub(crate) mod picture_look;
/// Smooth text and its contrast, beside the font's weight.
pub(crate) mod text_look;

/// The time zones to choose from: the system's, then each known one.
const ZONES: [&str; TIME_ZONES.len() + 1] = {
    let mut zones = [AUTO; TIME_ZONES.len() + 1];
    let mut i = 0;
    while i < TIME_ZONES.len() {
        zones[i + 1] = TIME_ZONES[i].0;
        i += 1;
    }
    zones
};

/// How the time is shown, and where it is.
static TIME: [SettingSpec; 4] = [
    SettingSpec::choice(
        settings::CLOCK_FORMAT.static_str(),
        "core.setting.clock-format",
        &[CLOCK_24H, CLOCK_12H],
        CLOCK_24H,
        groups::TIME,
    )
    .description("core.setting.clock-format.help"),
    SettingSpec::choice(
        settings::WEEK_START.static_str(),
        "core.setting.week-start",
        time::WEEK_STARTS,
        time::WEEK_STARTS[0],
        groups::TIME,
    )
    .description("core.setting.week-start.help"),
    SettingSpec::choice(
        settings::TIME_ZONE.static_str(),
        "core.setting.time-zone",
        &ZONES,
        AUTO,
        groups::TIME,
    )
    .description("core.setting.time-zone.help"),
    SettingSpec::text(
        settings::TIME_ZONE_RULE.static_str(),
        "core.setting.time-zone-rule",
        "",
        groups::TIME,
    )
    .advanced()
    .description("core.setting.time-zone-rule.help"),
];

/// Below which charge the battery shows when it shows only when low.
static BATTERY_LOW: [SettingSpec; 1] = [SettingSpec::number(
    settings::BATTERY_LOW.static_str(),
    "core.setting.battery-low",
    5,
    50,
    20,
    groups::STATUS_LINES,
)
.with_extra(&PERCENT_5)
.advanced()
.description("core.setting.battery-low.help")];

/// The line of the items of the place it is drawn in.
fn items_line<'e>(
    env: &crate::Place<'e>,
    frame: &crate::kit::status::StatusFrame<'_>,
) -> Option<crate::kit::status::StatusLine<'e>> {
    let place = env.status_items().place(frame.place?)?;
    let line = place.line(env, frame.area, frame.size, frame.color);
    Some(line.percent(frame.percent))
}

/// The time of day and the battery, for status lines; the line of a
/// place's items.
fn register_time_and_battery(r: &mut Staging) -> Result<(), RegisterError> {
    r.add_settings(&TIME)?;
    r.add_status_line(
        crate::kit::status::StatusLineSpec::new(
            paperpaw_sdk::builtin::status_lines::ITEMS,
            "core.status.line.items",
            |canvas, env, frame| {
                let (Some(line), Some(font)) =
                    (items_line(env, frame), env.fonts().role(frame.font))
                else {
                    return;
                };
                line.draw(canvas, env, &font, frame.context);
            },
        )
        .signature(|env, context| {
            let lists: Vec<&str> = env
                .status_items()
                .places()
                .flat_map(|p| p.lists(env))
                .collect();
            crate::kit::status::signature(env, &lists, context)
        })
        // The reader's line of items before it was the kernel's.
        .also(Id::new_static("reader.status-line.items"))
        .targets(|env, frame| {
            match (items_line(env, frame), env.fonts().role(frame.font)) {
                (Some(line), Some(font)) => line.targets(env, &font, frame.context),
                _ => Vec::new(),
            }
        }),
    )?;
    // The time and the battery give way after most others.
    r.add_status_item(
        StatusItemSpec::new(status_items::CLOCK, "core.status.clock", |env, _| {
            time::clock_text(env).map(StatusValue::Text)
        })
        .priority(160),
    )?;
    r.add_status_item(StatusItemSpec::new(
        status_items::DATE,
        "core.status.date",
        |env, _| time::date_text(env).map(StatusValue::Text),
    ))?;
    r.add_status_item(StatusItemSpec::new(
        status_items::DATE_SHORT,
        "core.status.date-short",
        |env, _| time::short_date_text(env).map(StatusValue::Text),
    ))?;
    r.add_status_item(
        StatusItemSpec::new(status_items::BATTERY, "core.status.battery", |env, _| {
            env.status().battery.map(|b| StatusValue::Level {
                percent: b.percent,
                charging: b.charging,
            })
        })
        .priority(192),
    )?;
    r.add_status_item(StatusItemSpec::new(
        status_items::BATTERY_LOW,
        "core.status.battery-low",
        |env, _| {
            let low = env.settings().read(&settings::BATTERY_LOW);
            env.status()
                .battery
                .filter(|b| i64::from(b.percent) <= low)
                .map(|b| StatusValue::Level {
                    percent: b.percent,
                    charging: b.charging,
                })
        },
    ))?;
    r.add_settings(&BATTERY_LOW)?;
    r.add_settings(&crate::awake::AWAKE_HOLDS)?;
    Ok(())
}

/// The key maps and the first-run setup, and the kernel's words.
fn register_keymaps_and_setup(r: &mut Staging) -> Result<(), RegisterError> {
    r.add_keymap(Cow::Borrowed(include_str!("../keymaps/default.ini")))?;
    r.add_keymap_preset(
        keymap_presets::TWO_BUTTONS,
        Cow::Borrowed(include_str!("../keymaps/two-buttons.ini")),
    )?;
    r.add_keymap_preset(
        keymap_presets::LEFT_HANDED,
        Cow::Borrowed(include_str!("../keymaps/left-handed.ini")),
    )?;
    // The first-run setup: the language, the buttons where there is
    // no touch, the time zone where the system has none.
    r.add_setup_step(
        crate::kit::setup::SetupStepSpec::new(
            Id::new_static("core.setup.language"),
            "",
            crate::kit::setup::SetupPage::Setting(settings::LANGUAGE.into()),
        )
        .order(10),
    )?;
    r.add_setup_step(
        crate::kit::setup::SetupStepSpec::new(
            Id::new_static("core.setup.buttons"),
            "",
            crate::kit::setup::SetupPage::Setting(settings::KEYMAP_PRESET.into()),
        )
        .order(20)
        .applies(|env: &crate::Place<'_>| env.device().input.buttons && !env.device().input.touch),
    )?;
    r.add_setup_step(
        crate::kit::setup::SetupStepSpec::new(
            Id::new_static("core.setup.time-zone"),
            "",
            crate::kit::setup::SetupPage::Setting(settings::TIME_ZONE.into()),
        )
        .help("core.setup.time-zone.help")
        .order(40)
        .applies(|env: &crate::Place<'_>| {
            env.status()
                .clock
                .is_none_or(|c| c.utc_offset_minutes.is_none())
        }),
    )?;
    crate::core_texts::add(r)?;
    Ok(())
}

/// Where a screenshot goes: `Screenshots` beside the library folder,
/// named by the time ("PaperPaw 2026-10-15 17-30-05.png").
fn screenshot_path(env: &crate::Place<'_>) -> alloc::string::String {
    let library = crate::kit::library::folder(env);
    let parent = paperpaw_sdk::files::parent(library);
    let dir = if parent.is_empty() {
        alloc::string::String::from("Screenshots")
    } else {
        alloc::format!("{parent}/Screenshots")
    };
    let _ = env.storage().create_dir(&dir);
    let name = match crate::kit::time::local_time(env) {
        Some(t) => alloc::format!(
            "PaperPaw {}-{:02}-{:02} {:02}-{:02}-{:02}",
            t.year,
            t.month,
            t.day,
            t.hour,
            t.minute,
            t.second
        ),
        None => alloc::format!("PaperPaw {}", env.now().0),
    };
    let mut path = alloc::format!("{dir}/{name}.png");
    let mut n = 2;
    while env.storage().exists(&path) {
        path = alloc::format!("{dir}/{name} ({n}).png");
        n += 1;
    }
    path
}

/// The size of the gap widget.
static SPACE: [SettingSpec; 1] = [SettingSpec::choice(
    widgets::SPACE_SIZE.static_str(),
    "core.setting.space-size",
    widgets::SPACE_SIZES,
    widgets::SPACE_SIZES[1],
    Id::new_static(widgets::SPACE),
)
.description("core.setting.space-size.help")];

/// The widgets for room between the others: a gap of a set height, and
/// the room left (see `paperpaw_sdk::widgets`).
fn register_spaces(r: &mut Staging) -> Result<(), RegisterError> {
    // As `widgets::settings_group` makes it, kept in the program (#248, B).
    static SPACE_GROUP: SettingGroupSpec =
        SettingGroupSpec::new(Id::new_static(widgets::SPACE), "core.widget.space");
    let space = Id::new_static(widgets::SPACE);
    r.add_setting_group(&SPACE_GROUP)?;
    r.add_settings(&SPACE)?;
    let space_height = |env: &crate::Place<'_>, _| {
        let size = env.settings().read(&widgets::SPACE_SIZE);
        let mm = match widgets::SPACE_SIZES.iter().position(|s| *s == size) {
            Some(0) => 4,
            Some(2) => 20,
            _ => 10,
        };
        env.metrics().mm(mm).min(u32::from(u16::MAX)) as u16
    };
    r.add_widget(
        WidgetSpec::drawn(
            space,
            "core.widget.space",
            Drawn::new(space_height, |_, _, _| {}),
        )
        .category(widgets::categories::LAYOUT),
    )?;
    // Screens that place widgets give it the room left.
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(widgets::FILL),
            "core.widget.fill",
            Drawn::new(|_, _| 0, |_, _, _| {}),
        )
        .category(widgets::categories::LAYOUT),
    )?;
    Ok(())
}

/// A hint that is a word of its own (a translation key).
const fn label(key: &'static str) -> ActionHint {
    ActionHint::Label(Cow::Borrowed(key))
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// Every plugin writes numbers with the language's decimal mark
    /// (`paperpaw_sdk::numbers`), which the kernel's catalogs name.
    #[test]
    fn languages_name_their_decimal_mark() {
        use paperpaw_sdk::numbers::DECIMAL_MARK;
        for (source, mark) in [
            (include_str!("../i18n/en.ini"), "."),
            (include_str!("../i18n/de.ini"), ","),
        ] {
            let catalog = Catalog::parse(source).unwrap();
            assert_eq!(catalog.get(DECIMAL_MARK), Some(mark));
        }
    }
}
