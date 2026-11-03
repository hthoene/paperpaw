//! Pomodoro with the real kernel and the home screen: the widgets show
//! the phase, the minutes and the day's count; the timer ends phases by
//! itself; the state survives a restart; hostile files change nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{Rotation, SystemStatus, WallClock};
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
const STATE: &str = ".paperpaw/plugins/pomodoro/state.txt";
/// The timer 2x2, the button 1x1 and the count 2x1.
const PAGE: &str = "pomodoro.timer 0/0 2x2, pomodoro.button 2/0 1x1, pomodoro.today 0/2 2x1";

struct Reader {
    kernel: Kernel,
    now: u64,
}

impl Reader {
    fn new(config: &str) -> Self {
        Self::with(config, PAGE, &[])
    }

    fn with(config: &str, page: &str, files: &[(&str, &str)]) -> Self {
        let mut storage = MemoryStorage::new();
        for (path, text) in files {
            storage = storage.with(path, text.as_bytes().to_vec());
        }
        let language = if config.contains("language = de") {
            ""
        } else {
            "[core]\nlanguage = en\n"
        };
        let user = format!(
            "{language}[help]\nshow = false\n[home]\ncolumns = 4\npage-1 = \"{page}\"\n{config}\n"
        );
        let mut kernel = Kernel::builder(X4)
            .storage(storage)
            .user_config(user)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(paperpaw_plugin_pomodoro::Pomodoro)
            .build()
            .unwrap();
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

    /// Lets `minutes` pass, waking whenever something is due.
    fn wait(&mut self, minutes: u64) {
        let until = self.now + minutes * 60_000;
        while self.now < until {
            self.now = (self.now + 20_000).min(until);
            let effects = self.kernel.tick(Instant(self.now));
            self.settle(effects);
        }
    }

    fn act(&mut self, action: &paperpaw_sdk::Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action.clone());
        self.settle(effects);
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .map(|p| format!("{}\n", p.label))
            .collect()
    }

    fn state(&self) -> String {
        let text = paperpaw_sdk::read_file(self.kernel.storage(), STATE, 256).unwrap_or_default();
        String::from_utf8(text).unwrap()
    }
}

#[test]
fn the_widgets_show_a_timer_ready_to_start() {
    let mut reader = Reader::new("");
    let shown = reader.shown();
    assert!(shown.contains("Focus"), "{shown}");
    assert!(shown.contains("25 min"), "{shown}");
    assert!(shown.contains("Tap to start"), "{shown}");
    assert!(shown.contains("Start"), "{shown}");
    assert!(shown.contains("0 rounds today"), "{shown}");
}

#[test]
fn a_tap_starts_and_the_minutes_count_down() {
    let mut reader = Reader::new("");
    reader.act(&paperpaw_plugin_pomodoro::TOGGLE);
    let shown = reader.shown();
    assert!(shown.contains("Pause"), "{shown}");
    assert!(!shown.contains("Tap to start"), "{shown}");
    reader.wait(10);
    let shown = reader.shown();
    assert!(shown.contains("15 min"), "{shown}");
    // Paused, it keeps the minutes.
    reader.act(&paperpaw_plugin_pomodoro::TOGGLE);
    reader.wait(5);
    let shown = reader.shown();
    assert!(shown.contains("15 min"), "{shown}");
    assert!(shown.contains("Paused. Tap to go on"), "{shown}");
    assert!(shown.contains("Resume"), "{shown}");
}

#[test]
fn a_round_ends_on_time_and_is_counted() {
    let mut reader = Reader::new("");
    reader.act(&paperpaw_plugin_pomodoro::TOGGLE);
    reader.wait(26);
    let shown = reader.shown();
    assert!(shown.contains("Short break"), "{shown}");
    assert!(shown.contains("5 min"), "{shown}");
    assert!(shown.contains("1 round today"), "{shown}");
    assert!(
        reader.state().starts_with("1|0|0|0|1|"),
        "{}",
        reader.state()
    );
}

#[test]
fn the_next_phase_starts_by_itself_when_asked() {
    let mut reader = Reader::new("[pomodoro]\nauto = true");
    reader.act(&paperpaw_plugin_pomodoro::TOGGLE);
    reader.wait(28);
    let shown = reader.shown();
    // The break is running: 5 minutes, 3 gone.
    assert!(shown.contains("Short break"), "{shown}");
    assert!(shown.contains("Pause"), "{shown}");
    assert!(shown.contains("2 min"), "{shown}");
}

#[test]
fn a_set_ends_with_the_long_break_and_the_goal_shows() {
    let config = "[pomodoro]\nauto = true\nrounds = 2\ngoal = 3\nshort = 1\nlong = 10\nfocus = 5";
    let mut reader = Reader::new(config);
    assert!(reader.shown().contains("0 of 3 rounds today"));
    reader.act(&paperpaw_plugin_pomodoro::TOGGLE);
    // Focus 5, short 1, focus 5: now the long break runs.
    reader.wait(12);
    let shown = reader.shown();
    assert!(shown.contains("Long break"), "{shown}");
    assert!(shown.contains("2 of 3 rounds today"), "{shown}");
}

#[test]
fn skip_goes_on_without_counting() {
    let mut reader = Reader::new("");
    reader.act(&paperpaw_plugin_pomodoro::SKIP);
    let shown = reader.shown();
    assert!(shown.contains("Short break"), "{shown}");
    assert!(shown.contains("0 rounds today"), "{shown}");
}

#[test]
fn a_restart_goes_on_by_the_clock() {
    let end = NOW + 10 * 60;
    let kept = format!("0|1|{end}|1500000|2|{}|1,0,0,0,0,0,4", NOW / 86_400 + 1);
    let mut reader = Reader::with("", PAGE, &[(STATE, &kept)]);
    let shown = reader.shown();
    assert!(shown.contains("10 min"), "{shown}");
    assert!(shown.contains("Pause"), "{shown}");
    // The local day (Berlin, UTC+2) is the one the file holds or the next.
    assert!(
        shown.contains("1 round today") || shown.contains("0 rounds today"),
        "{shown}"
    );
}

#[test]
fn a_round_that_ended_while_off_moves_on() {
    let kept = format!("0|1|{}|60000|0|0|0,0,0,0,0,0,0", NOW - 30);
    let mut reader = Reader::with("", PAGE, &[(STATE, &kept)]);
    let shown = reader.shown();
    assert!(shown.contains("Short break"), "{shown}");
    assert!(shown.contains("1 round today"), "{shown}");
}

#[test]
fn damaged_state_is_ignored() {
    for kept in [
        "garbage",
        "",
        "0|1|x|y",
        "\u{0}\u{1}\u{2}",
        "9|9|9|9|9|9|9,9",
    ] {
        let mut reader = Reader::with("", PAGE, &[(STATE, kept)]);
        let shown = reader.shown();
        assert!(shown.contains("25 min"), "{kept:?}: {shown}");
        assert!(shown.contains("0 rounds today"), "{kept:?}: {shown}");
    }
}

#[test]
fn a_small_place_shows_the_figure() {
    let mut reader = Reader::with("", "pomodoro.timer 0/0 1x1", &[]);
    let shown = reader.shown();
    assert!(shown.contains("25"), "{shown}");
    // No room for the hint or the dots.
    assert!(!shown.contains("Tap to start"), "{shown}");
}

#[test]
fn it_speaks_german() {
    let mut reader = Reader::with("[core]\nlanguage = de", PAGE, &[]);
    let shown = reader.shown();
    assert!(shown.contains("Fokus"), "{shown}");
    assert!(shown.contains("25 Min."), "{shown}");
    assert!(shown.contains("0 Runden heute"), "{shown}");
}

#[test]
fn the_widgets_are_in_the_catalogue() {
    let reader = Reader::new("");
    let widgets = reader.kernel.widgets();
    for id in ["pomodoro.timer", "pomodoro.button", "pomodoro.today"] {
        assert!(widgets.has(id), "{id}");
    }
}
