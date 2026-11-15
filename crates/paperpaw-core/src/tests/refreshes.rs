//! Refreshes: why the panel is refreshed, verdicts of
//! filters (a mode, grays, later), a mode set exactly, a screen's own
//! mode when it opens.

use super::*;
use paperpaw_sdk::Params;
use paperpaw_sdk::filters::{Refresh, RefreshCause, RefreshFilterSpec, RefreshVerdict};

const PAGE: Id = Id::new_static("r.page");
const SHEET: Id = Id::new_static("r.sheet");
const STATUS: Id = Id::new_static("r.status");
const EXACT: Id = Id::new_static("r.exact");
const GRAYS: Id = Id::new_static("r.grays");
const TURN: Id = Id::new_static("r.turn");
const LOCK: Id = Id::new_static("r.lock");
const OPEN: Id = Id::new_static("r.open");
const ITEM: Id = Id::new_static("r.item");
const READING: Id = Id::new_static("r.reading");

type Seen = Rc<RefCell<Vec<Refresh>>>;

/// A page that guards the device.
struct Lock;

impl Screen for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn guards(&self) -> bool {
        true
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        Page.on_action(action, cx)
    }
}

/// A page: its status line changes (`r.status`), a change it sets the
/// mode of exactly (`r.exact`), a change that wants grays (`r.grays`).
struct Page;

impl Screen for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == STATUS {
            cx.request_redraw_area(Rect::new(0, 0, 10, 5));
            cx.refresh_cause(RefreshCause::Status);
        } else if *action == EXACT {
            cx.request_redraw_area(Rect::new(0, 20, 10, 5));
            cx.request_refresh(RefreshMode::Full);
            cx.set_refresh(RefreshMode::Fast);
        } else if *action == GRAYS {
            cx.request_redraw_area(Rect::new(0, 40, 10, 5));
        } else if *action == ITEM {
            // A line of the screen, as a clock or a focus moved.
            cx.request_redraw_area(Rect::new(0, 60, 10, 5));
        } else if *action == TURN {
            // A page turned: the whole screen.
            cx.request_redraw();
        } else if *action == OPEN {
            // A screen opened, said to be a status line.
            cx.refresh_cause(RefreshCause::Status);
            cx.command(Command::OpenScreen(SHEET, paperpaw_sdk::Params::new()));
        } else {
            return Handled::No;
        }
        Handled::Yes
    }
}

struct Refreshing(Seen);

impl Plugin for Refreshing {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("r"), "Refreshing", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            PAGE,
            "page",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.add_screen(
            ScreenSpec::new(SHEET, "sheet", Box::new(|_, _| Box::new(Page)))
                .refresh_on_open(RefreshMode::Fast),
        )?;
        r.add_screen(ScreenSpec::new(
            LOCK,
            "lock",
            Box::new(|_, _| Box::new(Lock)),
        ))?;
        r.add_screen(ScreenSpec::new(
            READING,
            "reading",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.bind_slot(slots::HOME, PAGE, Priority::DEFAULT)?;
        r.bind_slot(slots::READER, READING, Priority::DEFAULT)?;
        let seen = self.0.clone();
        r.add_refresh_filter(RefreshFilterSpec::new(
            Id::new_static("r.filter"),
            "",
            Box::new(move |_, refresh| {
                seen.borrow_mut().push(refresh.clone());
                match refresh.cause {
                    // A status line changed: shown with what comes next;
                    // screens too, as far as the kernel lets it.
                    RefreshCause::Status | RefreshCause::Screens => RefreshVerdict::Defer,
                    _ if refresh.region.y == 40 => RefreshVerdict::Grays(true),
                    _ => RefreshVerdict::Keep,
                }
            }),
        ))
    }
}

fn refreshing() -> (Kernel, Seen) {
    let seen = Seen::default();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Refreshing(seen.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    seen.borrow_mut().clear();
    (kernel, seen)
}

/// Filters hear why the panel is refreshed: a screen opened, a status
/// line; a refresh deferred goes with the next one.
#[test]
fn a_deferred_refresh_goes_with_the_next() {
    let (mut kernel, seen) = refreshing();
    let effects = kernel.run_action(Instant(10), STATUS);
    assert_eq!(effects.refresh(), None);
    assert_eq!(seen.borrow()[0].cause, RefreshCause::Status);
    let effects = kernel.run_action(Instant(20), EXACT);
    let (region, mode) = effects.refresh().unwrap();
    assert_eq!(
        mode,
        RefreshMode::Fast,
        "set exactly, over the full requested"
    );
    assert!(
        region.contains(Point::new(1, 1)),
        "the status line: {region:?}"
    );
    assert!(region.contains(Point::new(1, 21)), "{region:?}");
    let effects = kernel.run_action(Instant(30), GRAYS);
    assert!(effects.grays, "a filter wanted grays");
}

/// A screen opened in the mode it asks for; the filters hear it was a
/// screen, on top, and how many partial refreshes were made since the
/// panel was last cleaned.
#[test]
fn a_screen_opens_in_its_own_mode() {
    let (mut kernel, seen) = refreshing();
    let effects = kernel.open_screen(Instant(10), SHEET, Params::new());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Fast)
    );
    let refresh = seen.borrow()[0].clone();
    assert_eq!(refresh.cause, RefreshCause::Screens);
    assert_eq!(refresh.screen, Some(SHEET));
    assert!(!refresh.periodic);
    let effects = kernel.open_screen(Instant(20), PAGE, Params::new());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
}

/// The periodic full refresh says it is one; a clearing refresh cleans
/// the panel as a full one does.
#[test]
fn the_periodic_full_refresh_says_so() {
    let (mut kernel, seen) = refreshing();
    kernel
        .set_setting(Instant(1), settings::FULL_REFRESH_INTERVAL.as_str(), "2")
        .unwrap();
    render(&mut kernel);
    let screen = kernel.framebuffer.bounds();
    kernel.frame.refresh.done(
        RefreshMode::Clear,
        screen,
        screen,
        crate::refresh::Intervals::default(),
    );
    seen.borrow_mut().clear();
    for at in [10, 20] {
        kernel.run_action(Instant(at), TURN);
        render(&mut kernel);
    }
    let seen = seen.borrow();
    assert_eq!(seen[0].since_full, 0);
    assert!(!seen[0].periodic);
    assert!(seen[1].periodic, "{:?}", seen[1]);
    assert_eq!(seen[1].mode, RefreshMode::Full);
}

/// Where the menus' full refresh is set (#255), small refreshes in a menu
/// count and the third flashes the whole screen; on the reading screen
/// (the reader's slot) they neither count nor flash.
#[test]
fn small_refreshes_flash_in_menus_but_never_while_reading() {
    let (mut kernel, seen) = refreshing();
    kernel
        .set_setting(Instant(1), crate::refresh::UI_FULL_REFRESH_INTERVAL, "3")
        .unwrap();
    // Screens open without a flash, as on the X4: what reading counted
    // would show in the menu.
    kernel
        .set_setting(Instant(1), settings::FLASH_SCREENS.as_str(), "false")
        .unwrap();
    kernel.open_slot(Instant(2), slots::READER, Params::new());
    render(&mut kernel);
    assert_eq!(kernel.active_screen(), Some(&READING));
    let screen = kernel.framebuffer.bounds();
    let modes = |kernel: &mut Kernel, start: u64| {
        seen.borrow_mut().clear();
        for at in 0..5 {
            kernel.run_action(Instant(start + at * 10), ITEM);
            render(kernel);
        }
        seen.borrow()
            .iter()
            .map(|r| (r.mode, r.region == screen))
            .collect::<Vec<_>>()
    };
    let small = (RefreshMode::Partial, false);
    let flash = (RefreshMode::Full, true);
    assert_eq!(modes(&mut kernel, 10), [small; 5], "reading");
    kernel.open_screen(Instant(100), PAGE, Params::new());
    render(&mut kernel);
    assert_eq!(kernel.active_screen(), Some(&PAGE));
    // Reading counted nothing: the third in the menu flashes, not the first.
    assert_eq!(
        modes(&mut kernel, 200),
        [small, small, flash, small, small],
        "in a menu"
    );
}

/// Refreshes are deferred at most three times in a row, and never while
/// a screen guards the device.
#[test]
fn refreshes_are_deferred_seldom() {
    let (mut kernel, _) = refreshing();
    for at in [10, 20, 30] {
        assert_eq!(kernel.run_action(Instant(at), STATUS).refresh(), None);
    }
    assert!(kernel.run_action(Instant(40), STATUS).refresh().is_some());
    kernel.open_screen(Instant(50), LOCK, Params::new());
    render(&mut kernel);
    assert!(
        kernel.run_action(Instant(60), STATUS).refresh().is_some(),
        "under the lock"
    );
}

/// A screen opened is refreshed, whatever cause a screen said.
#[test]
fn a_screen_opened_is_never_deferred() {
    let (mut kernel, _) = refreshing();
    let effects = kernel.run_action(Instant(10), OPEN);
    assert!(effects.refresh().is_some());
    assert_eq!(kernel.active_screen(), Some(&SHEET));
}

/// Counts the refreshes it sees.
struct Spy(Rc<core::cell::Cell<u32>>);

impl Plugin for Spy {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("spy"), "Spy", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let seen = self.0.clone();
        r.add_refresh_filter(RefreshFilterSpec::new(
            Id::new_static("spy.refresh"),
            "",
            Box::new(move |_, _| {
                seen.set(seen.get() + 1);
                RefreshVerdict::Keep
            }),
        ))
    }
}

/// While a screen guards the device, another plugin's refresh filters see
/// no refresh, and presses are not counted: neither tells how long a code
/// typed is.
#[test]
fn nothing_counts_what_is_typed_into_a_guard() {
    let seen = Rc::new(core::cell::Cell::new(0));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Refreshing(Seen::default()))
        .plugin(Spy(seen.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel.open_screen(Instant(10), LOCK, paperpaw_sdk::Params::new());
    render(&mut kernel);
    let (before, inputs) = (seen.get(), kernel.shared.usage.inputs);
    press(&mut kernel, 1000, "confirm");
    kernel.run_action(Instant(2000), STATUS);
    render(&mut kernel);
    assert_eq!(seen.get(), before);
    assert_eq!(kernel.shared.usage.inputs, inputs);
}

/// A valid home screen that draws only an overlay, as a plugin may do.
struct OverlayHome;
struct OverlayHomeScreen;

impl Screen for OverlayHomeScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(Rect::new(0, 45, 90, 45))
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 45, 90, 45, Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for OverlayHome {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("initial"), "Initial", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        const HOME: Id = Id::new_static("initial.home");
        r.add_screen(ScreenSpec::new(
            HOME,
            "Home",
            Box::new(|_, _| Box::new(OverlayHomeScreen)),
        ))?;
        r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)?;
        r.on(paperpaw_sdk::builtin::events::STARTED, |_, _, out| {
            out.push(Command::Sleep);
            out.push(Command::Restart);
        })
    }
}

#[test]
fn startup_overlay_replaces_every_row_and_preserves_start_effects() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayHome)
        .sleep(true)
        .frame_rows(10)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    assert!(effects.sleep() && effects.restart());
    assert_eq!(effects.refresh().unwrap().0, Rect::new(0, 0, 90, 90));
    let mut next_row = 0;
    kernel.render(&mut |top, rows| {
        assert_eq!(top, next_row);
        for y in 0..rows.height {
            let expected = if top + y < 45 {
                Color::WHITE
            } else {
                Color::BLACK
            };
            assert_eq!(rows.pixel(0, y), expected);
        }
        next_row += rows.height;
    });
    assert_eq!(next_row, 90);
}

#[test]
fn a_full_redraw_keeps_deferred_work_and_its_final_frame() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Opener)
        .busy_mark(true)
        .clock(still_clock)
        .frame_rows(10)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let deferred = press(&mut kernel, 100, "confirm");
    assert!(deferred.proceed());
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("opener.screen")
    );
    let effects = deferred.and(kernel.draw_again(Instant(151)));
    assert!(effects.proceed());
    // A shell with a stale panel skips the busy frame and proceeds directly.
    kernel.proceed(Instant(152));
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
    let mut next_row = 0;
    kernel.render(&mut |top, rows| {
        assert_eq!(top, next_row);
        next_row += rows.height;
    });
    assert_eq!(next_row, 90);
}

#[test]
fn two_small_items_far_apart_are_no_page_and_keep_a_picture_in_place() {
    let shown = Rc::new(RefCell::new(true));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(PicturePlugin(shown.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let mode = |effects: crate::Effects| effects.refresh();
    kernel.apply(Command::Redraw(RefreshMode::Partial));
    kernel.finish();
    render(&mut kernel);
    assert!(kernel.frame.pictures_shown);
    // The focus moves from an item at the top to one at the bottom: the
    // rectangle round them is the screen, what is drawn is two items. The
    // picture stays as it is: no full refresh to clear it, nor one of
    // the sixth step.
    let screen = kernel.framebuffer.bounds();
    for _ in 0..12 {
        let (w, h) = (screen.width / 5, screen.height / 5);
        kernel.invalidate(Rect::new(0, 0, w, h));
        kernel.invalidate(Rect::new(screen.width - w, screen.height - h, w, h));
        let (region, mode) = mode(kernel.finish()).unwrap();
        assert_eq!(region, screen);
        assert_eq!(mode, RefreshMode::Partial);
        render(&mut kernel);
    }
    // A whole screen drawn anew still is a page, and leaving the
    // picture still refreshes in full.
    *shown.borrow_mut() = false;
    kernel.apply(Command::Redraw(RefreshMode::Partial));
    assert_eq!(mode(kernel.finish()).map(|m| m.1), Some(RefreshMode::Full));
}

/// A page's pictures drawn after its text (`command.redraw_area`) are no
/// page turned, however large: their frame is not counted, nor is it the
/// one made the full refresh of the whole screen that is due.
#[test]
fn a_followup_of_most_of_the_screen_is_no_page_turned() {
    let (mut kernel, seen) = refreshing();
    kernel
        .set_setting(Instant(1), settings::FULL_REFRESH_INTERVAL.as_str(), "3")
        .unwrap();
    render(&mut kernel);
    let screen = kernel.framebuffer.bounds();
    kernel.frame.refresh.done(
        RefreshMode::Clear,
        screen,
        screen,
        crate::refresh::Intervals::default(),
    );
    // Two pages turned: the third is the full one.
    for at in [10, 20] {
        kernel.run_action(Instant(at), TURN);
        render(&mut kernel);
    }
    assert_eq!(kernel.frame.refresh.since_full(), 2);
    // The second page's pictures, three quarters of the screen.
    let key = kernel.shared.stack.last().unwrap().key;
    let (width, height) = (u32::from(screen.width), u32::from(screen.height) * 3 / 4);
    let area = crate::native_host::commands::Area::new(0, 0, width, height).unwrap();
    kernel.frame.followup = Some(crate::native_host::commands::Followup {
        screen: key,
        mode: Some(RefreshMode::Partial),
        areas: alloc::vec![area],
    });
    seen.borrow_mut().clear();
    let (region, mode) = kernel.tick(Instant(30)).refresh().unwrap();
    assert_eq!(mode, RefreshMode::Partial);
    assert_eq!(u32::from(region.height), height, "the pictures' box only");
    assert!(!seen.borrow()[0].periodic);
    render(&mut kernel);
    assert_eq!(kernel.frame.refresh.since_full(), 2);
    // The page turned next is.
    let (region, mode) = kernel.run_action(Instant(40), TURN).refresh().unwrap();
    assert_eq!((region, mode), (screen, RefreshMode::Full));
}

/// A follow-up of `command.redraw_area` held back by a filter is drawn as
/// the follow-up when it is drawn at last: its screen's draws are marked.
#[test]
fn a_deferred_followup_is_still_drawn_as_the_followup() {
    let (mut kernel, _) = refreshing();
    let key = kernel.shared.stack.last().unwrap().key;
    let area = crate::native_host::commands::Area::new(0, 0, 10, 5).unwrap();
    kernel.frame.followup = Some(crate::native_host::commands::Followup {
        screen: key,
        mode: Some(RefreshMode::Partial),
        areas: alloc::vec![area],
    });
    // The filter holds its refresh back (said to be a status line's).
    kernel.output.refresh_cause = Some(RefreshCause::Status);
    assert_eq!(kernel.tick(Instant(10)).refresh(), None);
    assert!(kernel.frame.followup.is_none());
    // Calls before it is drawn keep the frame the follow-up's.
    kernel.tick(Instant(20));
    assert_eq!(kernel.frame.followup_frame, Some(key));
    // A page turn takes it along: drawn with the marker.
    assert!(kernel.run_action(Instant(30), TURN).refresh().is_some());
    let rows = Rc::clone(&kernel.shared.stack_rows);
    let mut marked = false;
    kernel.render(&mut |_, _| marked |= rows.is_followup(key));
    assert!(marked, "the deferred follow-up drawn unmarked");
    assert!(!kernel.shared.stack_rows.is_followup(key));
    // Drawn: the next call ends it.
    kernel.tick(Instant(40));
    assert_eq!(kernel.frame.followup_frame, None);
}
