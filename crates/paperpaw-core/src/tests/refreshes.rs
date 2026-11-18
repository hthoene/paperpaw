//! Refreshes: a mode set exactly, a screen's own mode when it opens,
//! the periodic full refresh.

use super::*;
use crate::Staging;
use paperpaw_sdk::Params;

const PAGE: Id = Id::new_static("r.page");
const SHEET: Id = Id::new_static("r.sheet");
const STATUS: Id = Id::new_static("r.status");
const EXACT: Id = Id::new_static("r.exact");
const TURN: Id = Id::new_static("r.turn");
const LOCK: Id = Id::new_static("r.lock");
const ITEM: Id = Id::new_static("r.item");
const READING: Id = Id::new_static("r.reading");

/// A page that guards the device.
struct Lock;

impl crate::testing::Fake for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn flags(&self) -> u32 {
        crate::testing::GUARDS
    }

    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        Page.on_action(action, cx)
    }
}

/// A page: its status line changes (`r.status`), a change it sets the
/// mode of exactly (`r.exact`).
struct Page;

impl crate::testing::Fake for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        if *action == STATUS {
            cx.request_redraw_area(Rect::new(0, 0, 10, 5));
        } else if *action == EXACT {
            cx.request_redraw_area(Rect::new(0, 20, 10, 5));
            cx.request_refresh(RefreshMode::Full);
            cx.set_refresh(RefreshMode::Fast);
        } else if *action == ITEM {
            // A line of the screen, as a clock or a focus moved.
            cx.request_redraw_area(Rect::new(0, 60, 10, 5));
        } else if *action == TURN {
            // A page turned: the whole screen.
            cx.request_redraw();
        } else {
            return Handled::No;
        }
        Handled::Yes
    }
}

fn refreshing_items() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            PAGE,
            "page",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.add_fake(
            crate::testing::FakeSpec::new(SHEET, "sheet", Box::new(|_, _| Box::new(Page)))
                .refresh_on_open(RefreshMode::Fast),
        )?;
        r.add_fake(crate::testing::FakeSpec::new(
            LOCK,
            "lock",
            Box::new(|_, _| Box::new(Lock)),
        ))?;
        r.add_fake(crate::testing::FakeSpec::new(
            READING,
            "reading",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.bind_slot(slots::HOME, PAGE, Priority::DEFAULT)?;
        r.bind_slot(slots::READER, READING, Priority::DEFAULT)
    }
}

/// What [`refreshing_items`] registers under.
fn refreshing_manifest() -> Manifest {
    Manifest::new(Id::new_static("r"), "Refreshing", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn refreshing() -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(refreshing_manifest(), refreshing_items())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

/// A mode set exactly wins over the full refresh requested.
#[test]
fn a_mode_set_exactly_wins() {
    let mut kernel = refreshing();
    let effects = kernel.run_action(Instant(20), EXACT);
    let (region, mode) = effects.refresh().unwrap();
    assert_eq!(
        mode,
        RefreshMode::Fast,
        "set exactly, over the full requested"
    );
    assert!(region.contains(Point::new(1, 21)), "{region:?}");
}

/// A screen opened in the mode it asks for.
#[test]
fn a_screen_opens_in_its_own_mode() {
    let mut kernel = refreshing();
    let effects = kernel.open_screen(Instant(10), SHEET, Params::new());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Fast)
    );
    let effects = kernel.open_screen(Instant(20), PAGE, Params::new());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
}

/// A clearing refresh cleans the panel as a full one does: the periodic
/// full refresh counts from it.
#[test]
fn the_periodic_full_refresh_counts_from_a_clearing_one() {
    let mut kernel = refreshing();
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
    assert_eq!(kernel.frame.refresh.since_full(), 0);
    let modes: Vec<_> = [10, 20]
        .map(|at| {
            let mode = kernel.run_action(Instant(at), TURN).refresh().map(|r| r.1);
            render(&mut kernel);
            mode
        })
        .into();
    assert_eq!(modes, [Some(RefreshMode::Partial), Some(RefreshMode::Full)]);
}

/// Where the menus' full refresh is set (#255), small refreshes in a menu
/// count and the third flashes the whole screen; on the reading screen
/// (the reader's slot) they neither count nor flash.
#[test]
fn small_refreshes_flash_in_menus_but_never_while_reading() {
    let mut kernel = refreshing();
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
        (0..5)
            .map(|at| {
                let (region, mode) = kernel
                    .run_action(Instant(start + at * 10), ITEM)
                    .refresh()
                    .unwrap();
                render(kernel);
                (mode, region == screen)
            })
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

/// While a screen guards the device, presses are not counted: that would
/// tell how long a code typed is.
#[test]
fn nothing_counts_what_is_typed_into_a_guard() {
    let mut kernel = refreshing();
    kernel.open_screen(Instant(10), LOCK, paperpaw_sdk::Params::new());
    render(&mut kernel);
    let inputs = kernel.shared.usage.inputs;
    press(&mut kernel, 1000, "confirm");
    kernel.run_action(Instant(2000), STATUS);
    render(&mut kernel);
    assert_eq!(kernel.shared.usage.inputs, inputs);
}

struct OverlayHomeScreen;

impl crate::testing::Fake for OverlayHomeScreen {
    fn region(&self, _: &Place<'_>) -> Option<Rect> {
        Some(Rect::new(0, 45, 90, 45))
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.fill_rect(0, 45, 90, 45, Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A valid home screen that draws only an overlay, as a plugin may do.
fn overlay_home() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    |r| {
        const HOME: Id = Id::new_static("initial.home");
        r.add_fake(crate::testing::FakeSpec::new(
            HOME,
            "Home",
            Box::new(|_, _| Box::new(OverlayHomeScreen)),
        ))?;
        r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)?;
        r.on(paperpaw_sdk::builtin::events::STARTED, |k, p| {
            k.with_event(&paperpaw_sdk::builtin::events::STARTED, p, |_, _, out| {
                out.push(Ask::Sleep);
                out.push(Ask::Restart);
            });
        })
    }
}

/// What [`overlay_home`] registers under.
fn overlay_home_manifest() -> Manifest {
    Manifest::new(Id::new_static("initial"), "Initial", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

#[test]
fn startup_overlay_replaces_every_row_and_preserves_start_effects() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(overlay_home_manifest(), overlay_home())
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
        .items(crate::tests::opener_manifest(), crate::tests::opener())
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
        .items(
            crate::tests::picture_plugin_manifest(),
            crate::tests::picture_plugin(shown.clone()),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let mode = |effects: crate::Effects| effects.refresh();
    kernel.apply(Ask::Redraw(RefreshMode::Partial));
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
    kernel.apply(Ask::Redraw(RefreshMode::Partial));
    assert_eq!(mode(kernel.finish()).map(|m| m.1), Some(RefreshMode::Full));
}

/// A page's pictures drawn after its text (`command.redraw_area`) are no
/// page turned, however large: their frame is not counted, nor is it the
/// one made the full refresh of the whole screen that is due.
#[test]
fn a_followup_of_most_of_the_screen_is_no_page_turned() {
    let mut kernel = refreshing();
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
    let (region, mode) = kernel.tick(Instant(30)).refresh().unwrap();
    assert_eq!(mode, RefreshMode::Partial);
    assert_eq!(u32::from(region.height), height, "the pictures' box only");
    render(&mut kernel);
    assert_eq!(kernel.frame.refresh.since_full(), 2);
    // The page turned next is.
    let (region, mode) = kernel.run_action(Instant(40), TURN).refresh().unwrap();
    assert_eq!((region, mode), (screen, RefreshMode::Full));
}

/// Home after Back and a book opened are no page turns (#355): round trips
/// between them never count towards the periodic full refresh, so Back→Home
/// does not flash at every third return; six page turns still do.
#[test]
fn opening_and_leaving_a_book_is_no_page_turned() {
    let mut kernel = refreshing();
    kernel
        .set_setting(Instant(1), settings::FULL_REFRESH_INTERVAL.as_str(), "6")
        .unwrap();
    kernel
        .set_setting(Instant(1), settings::FLASH_SCREENS.as_str(), "false")
        .unwrap();
    render(&mut kernel);
    let screen = kernel.framebuffer.bounds();
    kernel.frame.refresh.done(
        RefreshMode::Clear,
        screen,
        screen,
        crate::refresh::Intervals::default(),
    );
    let mut at = 10;
    let mut modes = Vec::new();
    for _ in 0..8 {
        modes.push(kernel.open_slot(Instant(at), slots::READER, Params::new()));
        render(&mut kernel);
        kernel.close_top(paperpaw_sdk::CloseCause::Back);
        modes.push(kernel.tick(Instant(at + 5)));
        render(&mut kernel);
        at += 10;
    }
    let modes: Vec<_> = modes.iter().map(|e| e.refresh().map(|r| r.1)).collect();
    assert!(
        modes.iter().all(|m| *m == Some(RefreshMode::Partial)),
        "{modes:?}"
    );
    assert_eq!(kernel.frame.refresh.since_full(), 0);
    // Page turns on the book count as before: the sixth is the full one.
    kernel.open_slot(Instant(500), slots::READER, Params::new());
    render(&mut kernel);
    let turns: Vec<_> = (0..6)
        .map(|i| {
            let mode = kernel
                .run_action(Instant(600 + i * 10), TURN)
                .refresh()
                .map(|r| r.1);
            render(&mut kernel);
            mode
        })
        .collect();
    assert_eq!(turns[..5], [Some(RefreshMode::Partial); 5]);
    assert_eq!(turns[5], Some(RefreshMode::Full));
}
