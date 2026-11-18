//! The start and restart screens (the slots `core.start`, `core.restart`,
//! #219): the restart screen told why and dropped when it takes too long; the
//! start picture drawn ahead onto the card, shown only when it is whole and
//! of the screen's shape.

use core::cell::Cell;

use super::*;
use crate::Staging;
use crate::start_picture::{self, Bad, LIMIT_MS, PATH, Prepared};
use paperpaw_platform::Storage;
use paperpaw_sdk::builtin::params;

const SCREEN: Id = Id::new_static("shot.screen");

/// A screen with a black square at the top left, 10 pixels wide.
struct Shot;

impl crate::testing::Fake for Shot {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.clear(Color::WHITE);
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }
}

/// A clock the screen's opening moves on by `slow` milliseconds.
type Clock = Rc<Cell<u64>>;

fn plugin(
    kept: Log,
    clock: Clock,
    slow: u64,
    slot: Option<Id>,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let (log, clock) = (kept.clone(), clock.clone());
        r.add_fake(crate::testing::FakeSpec::new(
            SCREEN,
            "shot",
            Box::new(move |_, given| {
                let reason = given.get(params::OFF_REASON).unwrap_or("none");
                log.borrow_mut()
                    .push(alloc::format!("opened with {reason}"));
                clock.set(clock.get() + slow);
                Box::new(Shot)
            }),
        ))?;
        if let Some(slot) = &slot {
            r.bind_slot(slot.clone(), SCREEN, Priority::DEFAULT)?;
        }
        Ok(())
    }
}

fn make(log: &Log, clock: &Clock, slow: u64, slot: Option<Id>) -> Kernel {
    let manifest = Manifest::new(Id::new_static("shot"), "Shot", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING);
    let mut kernel = Kernel::builder(DEVICE)
        .items(manifest, plugin(log.clone(), clock.clone(), slow, slot))
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

/// The start picture shown for the test device's 90 × 90: its rows, or why
/// not, and how many were passed on.
fn shown(storage: &dyn Storage) -> (Result<(), Bad>, Vec<u8>) {
    let mut rows = Vec::new();
    let result = start_picture::show(storage, 90, 90, 32, &mut |top, view| {
        assert_eq!(usize::from(top) * 12, rows.len());
        rows.extend_from_slice(view.data);
        true
    });
    (result, rows)
}

#[test]
fn the_restart_screen_is_told_why_and_asks_nothing_of_the_power() {
    let (log, clock) = (log(), Clock::default());
    let mut kernel = make(&log, &clock, 0, Some(slots::RESTART));
    let time = clock.clone();
    let effects = kernel
        .restart_screen(Instant(5), &|| Instant(time.get()))
        .expect("a screen is bound to the slot");
    assert_eq!(*log.borrow(), ["opened with restart"]);
    assert!(matches!(
        effects.refresh(),
        Some((_, paperpaw_sdk::RefreshMode::Full))
    ));
    assert!(!effects.restart() && !effects.power_off() && !effects.sleep());
}

#[test]
fn a_slow_restart_screen_gives_way_to_the_devices_own() {
    let (log, clock) = (log(), Clock::default());
    let mut kernel = make(&log, &clock, LIMIT_MS + 1, Some(slots::RESTART));
    let time = clock.clone();
    assert!(
        kernel
            .restart_screen(Instant(0), &|| Instant(time.get()))
            .is_none()
    );
    // Without a screen for the slot: the device's own, nothing opened.
    let (log, clock) = (crate::tests::log(), Clock::default());
    let mut kernel = make(&log, &clock, 0, None);
    assert!(kernel.restart_screen(Instant(0), &|| Instant(0)).is_none());
    assert!(log.borrow().is_empty());
}

#[test]
fn the_start_picture_is_drawn_ahead_and_shown_whole() {
    let (log, clock) = (log(), Clock::default());
    let mut kernel = make(&log, &clock, 0, Some(slots::START));
    let time = clock.clone();
    let prepared = kernel.prepare_start_picture(Instant(0), &|| Instant(time.get()));
    assert_eq!(prepared, Prepared::Written);
    assert_eq!(*log.borrow(), ["opened with none"]);
    let (result, rows) = shown(kernel.storage());
    assert_eq!(result, Ok(()));
    // 90 pixels: 12 bytes a row, the panel's 1 for white. The square's
    // 10 pixels are black (0), the rest white, the 6 bits past the width 1.
    assert_eq!(rows.len(), 12 * 90);
    assert_eq!(rows[..2], [0x00, 0x3f]);
    assert_eq!(rows[11], 0xff);
    assert_eq!(rows[12 * 10 + 1], 0xff);
}

#[test]
fn a_bad_start_picture_shows_nothing() {
    let storage = paperpaw_sdk::MemoryStorage::new();
    assert_eq!(shown(&storage), (Err(Bad::Missing), Vec::new()));
    let header = b"P4\n90 90\n";
    let body = [0u8; 12 * 90];
    // Cut short, of another size, not a PBM: nothing passed on.
    storage.write(PATH, header).unwrap();
    storage.append(PATH, &body[..100]).unwrap();
    assert_eq!(shown(&storage), (Err(Bad::Shape), Vec::new()));
    storage.write(PATH, b"P4\n90 80\n").unwrap();
    storage.append(PATH, &body[..12 * 80]).unwrap();
    assert_eq!(shown(&storage), (Err(Bad::Shape), Vec::new()));
    storage.write(PATH, b"P5\n90 90\n").unwrap();
    storage.append(PATH, &body).unwrap();
    assert_eq!(shown(&storage), (Err(Bad::Shape), Vec::new()));
    // Whole: shown.
    storage.write(PATH, header).unwrap();
    storage.append(PATH, &body).unwrap();
    assert_eq!(shown(&storage).0, Ok(()));
}

#[test]
fn a_slow_start_screen_leaves_the_devices_own_and_unbound_removes_the_picture() {
    let (log, clock) = (log(), Clock::default());
    let mut kernel = make(&log, &clock, LIMIT_MS + 1, Some(slots::START));
    let storage = kernel.storage();
    storage.write(PATH, b"an old picture").unwrap();
    let time = clock.clone();
    let prepared = kernel.prepare_start_picture(Instant(0), &|| Instant(time.get()));
    assert_eq!(prepared, Prepared::Failed);
    assert!(!kernel.storage().exists(PATH));
    assert_eq!(shown(kernel.storage()).0, Err(Bad::Missing));
    // Nothing bound: the picture of a choice before goes.
    let (log, clock) = (crate::tests::log(), Clock::default());
    let mut kernel = make(&log, &clock, 0, None);
    kernel.storage().write(PATH, b"an old picture").unwrap();
    assert_eq!(
        kernel.prepare_start_picture(Instant(0), &|| Instant(0)),
        Prepared::Removed
    );
    assert!(!kernel.storage().exists(PATH) && log.borrow().is_empty());
}
