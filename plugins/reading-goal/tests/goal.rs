//! The reading goal with the real kernel, the reading statistics, the home
//! screen and the sleep screen: reading counted by the statistics reaches
//! the cat, whose mood and minutes show on the home screen and while the
//! device sleeps; without the statistics the widget says so.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{Rotation, SystemStatus, WallClock};
use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
};

const X4: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// 14 October 2026, 10:41 in Berlin (UTC+2), in Unix seconds.
const NOW: i64 = 1_791_967_260;
const BOOK: &str = "books/Der Process.txt";
const WIDGET: &str = "reading-goal.widget 0/0 2x2";

struct Reader {
    kernel: Kernel,
    now: u64,
}

impl Reader {
    fn new(config: &str, stats: bool) -> Self {
        Self::with_files(config, stats, &[])
    }

    /// [`Reader::new`] with `files` on the card besides the book.
    fn with_files(config: &str, stats: bool, files: &[(&str, &[u8])]) -> Self {
        let mut storage = MemoryStorage::new().with(
            BOOK,
            "Jemand mußte Josef K. verleumdet haben ...".repeat(50),
        );
        for (path, bytes) in files {
            storage = storage.with(path, bytes.to_vec());
        }
        let user = format!(
            "[core]\nlanguage = en\n[help]\nshow = false\n[home]\ncolumns = 4\npage-1 = \"{WIDGET}\"\n{config}\n"
        );
        let mut builder = Kernel::builder(X4)
            .storage(storage)
            .user_config(user)
            .sleep(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(paperpaw_plugin_sleep::Sleep)
            .plugin(paperpaw_plugin_reading_goal::ReadingGoal);
        if stats {
            builder = builder.plugin(paperpaw_plugin_stats::Stats);
        }
        let mut kernel = builder.build().unwrap();
        kernel.set_status(
            Instant(0),
            SystemStatus::new(
                Some(WallClock::new(Instant(0), NOW * 1000, Some(120))),
                None,
            ),
        );
        let effects = kernel.start(Instant(0));
        let mut reader = Self { kernel, now: 0 };
        reader.settle(effects);
        reader
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            match effects.wake_at() {
                Some(at) if at.0 <= self.now + 1000 => {
                    self.now = self.now.max(at.0);
                    effects = self.kernel.tick(Instant(self.now));
                }
                _ => return,
            }
        }
    }

    /// The reader shows `page` of the book at `seconds` after the start.
    fn read(&mut self, seconds: u64, page: u32) {
        self.now = self.now.max(seconds * 1000);
        let reading = events::Reading::new(BOOK.into(), String::new(), 0.0, 0, 0, String::new())
            .page(Some(page))
            .pages(Some(100));
        let event = if page == 1 {
            events::BOOK_OPENED
        } else {
            events::PAGE_TURNED
        };
        let effects =
            self.kernel
                .send_event(Instant(self.now), event.id().clone(), reading.to_params());
        self.settle(effects);
    }

    /// Reads a page a minute for `minutes`, from page 1, from now on.
    fn read_for(&mut self, minutes: u32) {
        let start = self.now / 1000 + 10;
        for page in 1..=minutes + 1 {
            self.read(start + u64::from(page - 1) * 60, page);
        }
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .map(|p| format!("{}\n", p.label))
            .collect()
    }
}

#[test]
fn the_cat_follows_today_s_reading() {
    let mut reader = Reader::new("", true);
    let shown = reader.shown();
    assert!(shown.contains("A sad cat"), "{shown}");
    assert!(shown.contains("0 / 20 min"), "{shown}");

    reader.read_for(5);
    reader.run_home();
    let shown = reader.shown();
    assert!(shown.contains("5 / 20 min"), "{shown}");
    assert!(shown.contains("so-so"), "{shown}");

    reader.read_for(15);
    reader.run_home();
    let shown = reader.shown();
    assert!(shown.contains("20 / 20 min"), "{shown}");
    assert!(shown.contains("A happy cat"), "{shown}");
}

#[test]
fn thresholds_and_the_goal_are_settings() {
    let mut reader = Reader::new(
        "[stats]\ngoal = stats.goal.10\n[reading-goal]\ncontent = 40\nhappy = 150",
        true,
    );
    reader.read_for(5);
    reader.run_home();
    let shown = reader.shown();
    assert!(shown.contains("5 / 10 min"), "{shown}");
    assert!(shown.contains("A content cat"), "{shown}");
}

#[test]
fn without_the_statistics_it_says_what_it_needs() {
    let mut reader = Reader::new("", false);
    let shown = reader.shown();
    assert!(shown.contains("Statistics off"), "{shown}");
    assert!(shown.contains("A sad cat"), "{shown}");
}

#[test]
fn the_cat_is_a_sleep_screen_too() {
    let mut reader = Reader::new("[sleep]\nshow = reading-goal.sleep", true);
    reader.read_for(12);
    reader.now += 100;
    let effects = reader
        .kernel
        .run_action(Instant(reader.now), actions::SLEEP);
    reader.settle(effects);
    let shown = reader.shown();
    assert!(shown.contains("12 / 20 min"), "{shown}");
    assert!(shown.contains("A content cat"), "{shown}");
}

impl Reader {
    /// Back on the home screen, its widgets updated.
    fn run_home(&mut self) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), actions::HOME);
        self.settle(effects);
    }
}

/// One's own pictures, where the folder has one for the mood; the cat
/// for those it lacks.
#[test]
fn own_pictures_take_the_cats_place() {
    let logo: &[u8] = include_bytes!("../../about/logo.png");
    let files: [(&str, &[u8]); 1] = [("my-cats/sad.png", logo)];
    let config = "[reading-goal]\npictures = /my-cats/";
    let mut reader = Reader::with_files(config, true, &files);
    reader.shown();
    let own = format!(".paperpaw/plugins/reading-goal/own-sad-{}.gray", logo.len());
    assert!(reader.kernel.storage().exists(&own));
    reader.read_for(3);
    reader.run_home();
    let shown = reader.shown();
    assert!(shown.contains("so-so"), "{shown}");
    assert!(
        !reader
            .kernel
            .storage()
            .exists(".paperpaw/plugins/reading-goal/own-meh-0.gray")
    );
}

/// Without a goal (set to none), the minutes read alone.
#[test]
fn without_a_goal_the_minutes_read_show() {
    let mut reader = Reader::new("[stats]\ngoal = stats.goal.off", true);
    reader.read_for(4);
    reader.run_home();
    let shown = reader.shown();
    assert!(shown.contains("4 min\n"), "{shown}");
}

/// Where the statistics are off, the cat offers to switch them on: on at
/// the next start, which follows at once.
#[test]
fn the_statistics_are_offered_where_they_are_off() {
    let mut reader = Reader::new("[plugins]\nstats = false", true);
    assert!(reader.shown().contains("Statistics off"));
    reader.now += 100;
    let picked = paperpaw_sdk::ui::choice::Picked::new(String::new(), "switch-on".into());
    let effects = reader.kernel.send_event(
        Instant(reader.now),
        paperpaw_sdk::Id::new_static("reading-goal.offered"),
        picked.to_params(),
    );
    assert!(effects.restart(), "started anew");
    let config = paperpaw_sdk::read_file(reader.kernel.storage(), "paperpaw.ini", 1 << 16).unwrap();
    let config = String::from_utf8(config).unwrap();
    assert!(config.contains("stats = on"), "{config}");
}

/// While a book is open, today's reading is not written at every page
/// turned: when the device sleeps (or the book closes).
#[test]
fn today_is_written_when_the_device_sleeps() {
    let mut reader = Reader::new("", true);
    let file = ".paperpaw/plugins/reading-goal/today.txt";
    let minutes = |reader: &Reader| {
        let text = paperpaw_sdk::read_file(reader.kernel.storage(), file, 64).unwrap_or_default();
        let text = String::from_utf8(text).unwrap();
        let seconds = text
            .trim()
            .split_once(' ')
            .map(|(_, s)| s.parse::<u32>().unwrap());
        seconds.unwrap_or(0) / 60
    };
    reader.read_for(5);
    assert_eq!(minutes(&reader), 0, "written while pages turned");
    reader.now += 100;
    let effects = reader.kernel.suspend(Instant(reader.now));
    reader.settle(effects);
    assert_eq!(minutes(&reader), 5);
}
