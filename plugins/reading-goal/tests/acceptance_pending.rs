//! Native storage acceptance and pending image/UI composition fixtures.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "fixture failures fail the regression tests"
)]

#[cfg(feature = "linked")]
#[path = "../../test-support/hosted.rs"]
mod hosted;

#[test]
#[cfg(feature = "linked")]
fn the_statistics_tell_today_and_it_is_written_to_the_card_when_a_book_closes() {
    use paperpaw_sdk::{
        EventData, Instant, MemoryStorage, SystemStatus, WallClock, builtin::events,
    };
    let mut page = [0u8; 16];
    page[2..6].copy_from_slice(&1u32.to_le_bytes());
    page[6..10].copy_from_slice(&(20_000u32 * 86_400).to_le_bytes());
    page[10..12].copy_from_slice(&750u16.to_le_bytes());
    page[12..16].copy_from_slice(&10u32.to_le_bytes());
    let setup = hosted::Setup::x4().with_kernel(move |b| {
        b.storage(MemoryStorage::new().with(".paperpaw/plugins/stats/pages.bin", page))
    });
    let mut goal = hosted::install_in(
        &setup,
        paperpaw_plugin_reading_goal::NATIVE_ENTRY.entry(),
        "reading-goal",
        &["core.registry", "core.ui"],
    )
    .unwrap();
    hosted::also(
        &mut goal,
        paperpaw_plugin_stats::NATIVE_ENTRY.entry(),
        "stats",
        &[
            "core.library",
            "core.net",
            "core.registry",
            "core.secrets",
            "core.ui",
        ],
        paperpaw_core::native_host::FixedTables::NONE,
    )
    .unwrap();
    let _ = goal.kernel().start(Instant(0));
    let _ = goal.kernel().set_status(
        Instant(0),
        SystemStatus::new(
            Some(WallClock::new(Instant(0), 20_000 * 86_400_000, Some(0))),
            None,
        ),
    );
    let reading = events::Reading::new("book.epub".into(), "Book".into(), 0.0, 0, 0, "".into());
    let _ = goal.kernel().send_event(
        Instant(0),
        events::BOOK_OPENED.id().clone(),
        reading.to_params(),
    );
    let _ = goal.kernel().send_event(
        Instant(0),
        events::BOOK_CLOSED.id().clone(),
        reading.to_params(),
    );
    assert!(goal.dropped().is_empty(), "{:?}", goal.dropped());
    let mut bytes = [0; 64];
    let n = goal
        .kernel()
        .storage()
        .read_at(".paperpaw/plugins/reading-goal/today.txt", 0, &mut bytes)
        .unwrap_or_else(|error| {
            panic!(
                "stage did not write: {error:?}; warnings={:?}",
                goal.kernel().take_warnings()
            )
        });
    assert_eq!(&bytes[..n], b"20000 750\n");
    assert!(goal.dropped().is_empty(), "{:?}", goal.dropped());
}

#[cfg(feature = "linked")]
mod data_folder {
    use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
    use std::{cell::Cell, rc::Rc};

    pub const TODAY: &str = ".paperpaw/plugins/reading-goal/today.txt";

    #[derive(Default)]
    pub struct Counts {
        pub reads: Cell<u32>,
        pub writes: Cell<u32>,
    }

    pub struct Card(pub MemoryStorage, pub Rc<Counts>);

    impl Storage for Card {
        fn read_at(&self, path: &str, offset: u64, out: &mut [u8]) -> Result<usize, StorageError> {
            if path == TODAY {
                self.1.reads.set(self.1.reads.get() + 1);
            }
            self.0.read_at(path, offset, out)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.0.size(path)
        }
        fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StorageError> {
            if path == TODAY {
                self.1.writes.set(self.1.writes.get() + 1);
            }
            self.0.write(path, bytes)
        }
        fn append(&self, path: &str, bytes: &[u8]) -> Result<(), StorageError> {
            self.0.append(path, bytes)
        }
        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.0.remove(path)
        }
    }
}

#[test]
#[cfg(feature = "linked")]
fn today_txt_is_read_and_replaced_only_when_it_differs() {
    use data_folder::{Card, Counts, TODAY};
    use paperpaw_sdk::{
        EventData, Instant, MemoryStorage, SystemStatus, WallClock, builtin::events,
    };
    use std::rc::Rc;

    for (old, writes) in [("20000 750\n", 0), ("19999 3600\n", 1)] {
        let counts = Rc::new(Counts::default());
        let observed = counts.clone();
        let mut page = [0u8; 16];
        page[2..6].copy_from_slice(&1u32.to_le_bytes());
        page[6..10].copy_from_slice(&(20_000u32 * 86_400).to_le_bytes());
        page[10..12].copy_from_slice(&750u16.to_le_bytes());
        page[12..16].copy_from_slice(&10u32.to_le_bytes());
        let setup = hosted::Setup::x4().with_kernel(move |builder| {
            builder.storage(Card(
                MemoryStorage::new()
                    .with(".paperpaw/plugins/stats/pages.bin", page)
                    .with(TODAY, old.as_bytes()),
                counts.clone(),
            ))
        });
        let mut goal = hosted::install_in(
            &setup,
            paperpaw_plugin_reading_goal::NATIVE_ENTRY.entry(),
            "reading-goal",
            &["core.registry", "core.ui"],
        )
        .unwrap();
        hosted::also(
            &mut goal,
            paperpaw_plugin_stats::NATIVE_ENTRY.entry(),
            "stats",
            &[
                "core.library",
                "core.net",
                "core.registry",
                "core.secrets",
                "core.ui",
            ],
            paperpaw_core::native_host::FixedTables::NONE,
        )
        .unwrap();
        let _ = goal.kernel().start(Instant(0));
        let _ = goal.kernel().set_status(
            Instant(0),
            SystemStatus::new(
                Some(WallClock::new(Instant(0), 20_000 * 86_400_000, Some(0))),
                None,
            ),
        );
        let reading = events::Reading::new("book.epub".into(), "Book".into(), 0.0, 0, 0, "".into());
        for _ in 0..2 {
            let _ = goal.kernel().send_event(
                Instant(0),
                events::BOOK_OPENED.id().clone(),
                reading.to_params(),
            );
            let _ = goal.kernel().send_event(
                Instant(0),
                events::BOOK_CLOSED.id().clone(),
                reading.to_params(),
            );
        }
        assert!(goal.dropped().is_empty(), "{:?}", goal.dropped());
        assert!(
            observed.reads.get() >= 1,
            "existing data-folder file must be read"
        );
        assert_eq!(
            observed.writes.get(),
            writes,
            "only replace a different figure"
        );
        let mut bytes = [0; 64];
        let n = goal
            .kernel()
            .storage()
            .read_at(TODAY, 0, &mut bytes)
            .unwrap();
        assert_eq!(&bytes[..n], b"20000 750\n");
    }
}

#[test]
#[ignore = "needs an installed-asset image/decode-and-draw fixture, absent from hosted.rs"]
fn the_widget_shows_the_cat_of_todays_mood_and_the_minutes() {
    panic!("pending end-to-end fixture: installed mood picture and minute text");
}

#[test]
#[ignore = "needs a native home/settings composition fixture: hosted.rs installs entries but does not arrange/draw widgets or route taps into core.settings; scripted_offer.rs checks only dialog commands"]
fn the_widget_is_drawn_and_a_tap_opens_the_goal() {
    panic!("pending end-to-end fixture: widget frame, tap, and stats.goal focus");
}

#[test]
#[ignore = "needs a native sleep-slot plus installed-image composition fixture; hosted.rs does not drive sleep selection and image pumping"]
fn the_sleep_screen_shows_the_cat_large_in_the_middle() {
    panic!("pending end-to-end fixture: selected sleep screen with centred cat and minutes");
}
