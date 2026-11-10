//! Reviewing flash cards with the real kernel and the default fonts: with
//! buttons alone and by touch, the answers kept and written into the
//! deck as the review closes, the widget counting what is due.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation, SystemStatus, WallClock,
};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::{Part, Role};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage,
};

/// An X4: black and white, buttons.
const BUTTONS: DeviceInfo = DeviceInfo::new(
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

/// A touch screen as large.
const TOUCH: DeviceInfo = BUTTONS
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

const DIR: &str = ".paperpaw/plugins/vocabulary/cards";
/// The time the device knows, in seconds since 1970.
const NOW: i64 = 1_791_967_260;

/// A deck of two new cards and one due yesterday.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new();
    let write = |name: &str, text: &str| {
        storage
            .write(&format!("{DIR}/{name}"), text.as_bytes())
            .unwrap();
    };
    write("decks.txt", "paperpaw-decks 1\n1\tWords\n");
    let yesterday = NOW - 24 * 3600;
    let seen = NOW - 4 * 24 * 3600;
    write(
        "deck-1.txt",
        &format!(
            "paperpaw-cards 1\n\
             1\t0\t0\t0\t0\t0\t0\tHund\tdog\tDer Hund bellt.\n\
             2\t0\t0\t0\t0\t0\t0\tKatze\tcat\t\n\
             3\t{yesterday}\t{seen}\t317\t528\t1\t0\tMaus\tmouse\t\n"
        ),
    );
    write(
        "due.txt",
        &format!("paperpaw-due 1\n1\t2\t{}=1\n", yesterday / 3600),
    );
    storage
}

struct Reader {
    kernel: Kernel,
    now: u64,
}

impl Reader {
    fn new(device: DeviceInfo, storage: MemoryStorage, config: &str) -> Self {
        Self::built(device, storage, config, false)
    }

    /// A reader that sleeps, with the sleep screen.
    fn sleeping(storage: MemoryStorage, config: &str) -> Self {
        Self::built(BUTTONS, storage, config, true)
    }

    fn built(device: DeviceInfo, storage: MemoryStorage, config: &str, sleeps: bool) -> Self {
        let user = format!("[core]\nlanguage = en\n[home]\ngesture-tips = false\n{config}\n");
        let mut builder = Kernel::builder(device)
            .storage(storage)
            .user_config(user)
            .sleep(sleeps)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(paperpaw_plugin_vocabulary::Vocabulary);
        if sleeps {
            builder = builder.plugin(paperpaw_plugin_sleep::Sleep);
        }
        let mut kernel = builder.build().unwrap();
        kernel.set_status(
            Instant(0),
            SystemStatus::new(Some(WallClock::new(Instant(0), NOW * 1000, Some(0))), None),
        );
        let effects = kernel.start(Instant(0));
        let mut reader = Self { kernel, now: 0 };
        reader.settle(effects);
        reader
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..1000 {
            match effects.wake_at() {
                Some(at) if at.0 <= self.now + 1000 => {
                    self.now = self.now.max(at.0);
                    effects = self.kernel.tick(Instant(self.now));
                }
                _ => return,
            }
        }
    }

    fn open(&mut self, screen: &str) {
        self.now += 100;
        let effects =
            self.kernel
                .open_screen(Instant(self.now), Id::parse(screen).unwrap(), Params::new());
        self.settle(effects);
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// Lets `millis` pass, the device left alone.
    fn wait(&mut self, millis: u64) {
        self.now += millis;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
    }

    fn parts(&mut self) -> Vec<Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    fn shown(&mut self) -> String {
        self.parts()
            .iter()
            .map(|p| format!("{}\n", p.label))
            .collect()
    }

    fn tap(&mut self, label: &str) {
        let part = self
            .parts()
            .into_iter()
            .find(|p| p.role == Role::Button && p.label == label)
            .unwrap_or_else(|| panic!("no button {label}"));
        let a = part.area;
        let at = Point::new(
            i32::from(a.x) + i32::from(a.width) / 2,
            i32::from(a.y) + i32::from(a.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 50;
            let event =
                InputEvent::Pointer(PointerEvent::new(0, phase, at, PointerTool::Finger, 1000));
            let effects = self.kernel.handle(Instant(self.now), event);
            // Not while the finger is down: that would be a long press.
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    fn deck(&self) -> String {
        let storage = self.kernel.storage();
        let file = format!("{DIR}/deck-1.txt");
        let size = storage.size(&file).unwrap() as usize;
        let bytes = paperpaw_sdk::read_file(storage, &file, size).unwrap();
        String::from_utf8(bytes).unwrap()
    }
}

#[test]
fn cards_are_reviewed_with_buttons_alone() {
    let mut reader = Reader::new(BUTTONS, card(), "");
    reader.open("vocabulary.review");
    // The card due first, its back hidden.
    let shown = reader.shown();
    assert!(
        shown.contains("Maus\n") && !shown.contains("mouse"),
        "{shown}"
    );
    assert!(shown.contains("3 left"), "{shown}");
    reader.run(actions::CONFIRM);
    let shown = reader.shown();
    assert!(shown.contains("mouse"), "{shown}");
    for answer in ["Again", "Hard", "Good", "Easy"] {
        assert!(shown.contains(&format!("{answer}\n")), "{shown}");
    }
    // When each would come again; the focus on Good.
    assert!(shown.contains("10 min"), "{shown}");
    let focused = reader.parts().into_iter().find(|p| p.focused).unwrap();
    assert_eq!(focused.label, "Good");
    reader.run(actions::CONFIRM);
    // Then the new ones, in their order; Again brings one back, once its
    // 10 minutes are over.
    assert!(reader.shown().contains("Hund\n"));
    reader.run(actions::CONFIRM);
    reader.run(actions::LEFT);
    reader.run(actions::LEFT);
    reader.run(actions::CONFIRM);
    assert!(reader.shown().contains("Katze\n"));
    reader.run(actions::CONFIRM);
    reader.run(actions::CONFIRM);
    reader.wait(10 * 60 * 1000);
    assert!(reader.shown().contains("Hund\n"), "Again: Hund once more");
    reader.run(actions::CONFIRM);
    reader.run(actions::CONFIRM);
    assert!(reader.shown().contains("Done: 4 cards reviewed."));
    // Kept as answered, written into the deck as the review closes.
    let storage = reader.kernel.storage();
    assert!(storage.exists(&format!("{DIR}/answers.txt")));
    reader.run(actions::BACK);
    assert!(
        !reader
            .kernel
            .storage()
            .exists(&format!("{DIR}/answers.txt"))
    );
    let deck = reader.deck();
    let reps: Vec<&str> = deck
        .lines()
        .skip(1)
        .filter(|l| !l.starts_with("next\t"))
        .map(|l| l.split('\t').nth(5).unwrap())
        .collect();
    assert_eq!(reps, ["2", "1", "2"], "{deck}");
}

/// A card answered Again waits the 10 minutes its button says: the
/// cards after it come in their order, then the screen says when it
/// comes, and shows it by itself once it is due.
#[test]
fn again_waits_its_ten_minutes() {
    let mut reader = Reader::new(BUTTONS, card(), "");
    reader.open("vocabulary.review");
    assert!(reader.shown().contains("Maus\n"));
    reader.run(actions::CONFIRM);
    reader.run(actions::LEFT);
    reader.run(actions::LEFT);
    reader.run(actions::CONFIRM);
    // Not at once: the next ones first, in their order.
    for next in ["Hund\n", "Katze\n"] {
        let shown = reader.shown();
        assert!(shown.contains(next), "{next}: {shown}");
        reader.run(actions::CONFIRM);
        reader.run(actions::CONFIRM);
    }
    let shown = reader.shown();
    assert!(shown.contains("Next card in 10 min."), "{shown}");
    assert!(!shown.contains("Maus"), "{shown}");
    // Not before its time.
    reader.wait(8 * 60 * 1000);
    assert!(!reader.shown().contains("Maus"), "{}", reader.shown());
    // Then by itself.
    reader.wait(2 * 60 * 1000);
    let shown = reader.shown();
    assert!(shown.contains("Maus\n"), "{shown}");
    assert!(shown.contains("1 left"), "{shown}");
}

#[test]
fn cards_are_reviewed_by_touch() {
    let mut reader = Reader::new(TOUCH, card(), "");
    reader.open("vocabulary.review");
    reader.tap("Show answer");
    let shown = reader.shown();
    assert!(shown.contains("mouse"), "{shown}");
    reader.tap("Easy");
    assert!(reader.shown().contains("Hund\n"));
    // New cards a day: none.
    let mut reader = Reader::new(TOUCH, card(), "[vocabulary]\nnew-per-day = 0");
    reader.open("vocabulary.review");
    reader.tap("Show answer");
    reader.tap("Good");
    assert!(reader.shown().contains("Done: 1 card reviewed."));
}

#[test]
fn the_widget_counts_the_cards_due() {
    let mut reader = Reader::new(
        BUTTONS,
        card(),
        "columns = 4\npage-1 = \"vocabulary.widget.due 0/0 4x2\"",
    );
    let shown = reader.shown();
    assert!(shown.contains("3 cards due"), "{shown}");
    let mut empty = Reader::new(
        BUTTONS,
        MemoryStorage::new(),
        "columns = 4\npage-1 = \"vocabulary.widget.due 0/0 4x2\"",
    );
    let shown = empty.shown();
    assert!(shown.contains("flash cards"), "{shown}");
}

#[test]
fn flash_cards_show_what_is_due_and_the_decks() {
    let mut reader = Reader::new(BUTTONS, card(), "");
    reader.open("vocabulary.cards");
    let shown = reader.shown();
    assert!(shown.contains("3 cards due"), "{shown}");
    assert!(shown.contains("Edit with your phone"), "{shown}");
    assert!(shown.contains("Words, 3 cards"), "{shown}");
}

impl Reader {
    fn sleep_and_wake(&mut self) -> String {
        self.run(actions::SLEEP);
        let asleep = self.shown();
        self.now += 60_000;
        let effects = self.kernel.resume(Instant(self.now));
        self.settle(effects);
        asleep
    }

    fn on_top(&self) -> String {
        self.kernel
            .active_screen()
            .map(|id| id.as_str().to_string())
            .unwrap_or_default()
    }
}

/// A flash card due as the sleep screen: its front while asleep, and on
/// waking the review with that card, its back on Select.
#[test]
fn a_card_due_is_the_sleep_screen_and_answered_on_waking() {
    let mut reader = Reader::sleeping(card(), "[sleep]\nshow = vocabulary.sleep-card");
    let asleep = reader.sleep_and_wake();
    assert!(asleep.contains("Maus\n"), "{asleep}");
    assert!(asleep.contains("3 cards due: wake to answer"), "{asleep}");
    assert_eq!(reader.on_top(), "vocabulary.review");
    assert!(reader.shown().contains("Maus\n"));
    reader.run(actions::CONFIRM);
    assert!(reader.shown().contains("mouse"));
}

/// Reviewing on waking is a setting, off at first.
#[test]
fn the_review_opens_on_waking_where_wanted() {
    let mut reader = Reader::sleeping(card(), "");
    reader.sleep_and_wake();
    assert_ne!(reader.on_top(), "vocabulary.review");
    let mut reader = Reader::sleeping(card(), "[vocabulary]\nreview-on-wake = true");
    reader.sleep_and_wake();
    assert_eq!(reader.on_top(), "vocabulary.review");
    // Nothing due: nothing opens.
    let mut reader = Reader::sleeping(MemoryStorage::new(), "[vocabulary]\nreview-on-wake = true");
    reader.sleep_and_wake();
    assert_ne!(reader.on_top(), "vocabulary.review");
}
