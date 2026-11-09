//! Vocabulary's declarations as borrowed builders, shared by the build script
//! (which writes them into the plugin image) and the tests.
//!
//! Part of the port: the eight settings, the start event, and the flash cards
//! (their action, place in the Tools menu, icon, and the screens of the decks
//! and of the review). The role of the words marked in books (seven more
//! settings), the word list, the phone page, the SD card's screens, the sleep
//! card and the widget follow with their host pieces (see `PORTING-7.md`): a
//! screen or an entry that cannot work is not declared.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, Kind, MenuEntryDecl, ScreenDecl,
    SpecDecl,
};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;
/// The action that opens the flash cards.
pub const CARDS_TOKEN: u32 = 2;
/// The minute tick.
pub const TICK_TOKEN: u32 = 3;
/// The action that opens the list of words.
pub const OPEN_TOKEN: u32 = 4;
/// The screen of the decks, the review's, and the list of words.
pub const DECKS_TOKEN: u32 = 10;
pub const REVIEW_TOKEN: u32 = 11;
pub const WORDS_TOKEN: u32 = 12;

/// The reading settings' group, where the settings are.
pub const GROUP: &str = "core.group.reading";
/// The dictionary's part of the reading settings, and the flash cards'.
const DICTIONARY: &str = "dictionary.section";
const CARDS: &str = "vocabulary.cards.title";
/// The folder the decks' files are in, unless the user chose one.
pub const DEFAULT_FOLDER: &str = "Flashcards";

/// Which settings there are: name, kind, advanced, part of the group, unit.
type Row = (
    &'static str,
    Kind<'static>,
    bool,
    &'static str,
    &'static str,
);

const ROWS: [Row; 8] = [
    ("collect", Kind::Bool(true), true, DICTIONARY, ""),
    ("mark", Kind::Bool(false), false, DICTIONARY, ""),
    (
        "new-per-day",
        Kind::Int {
            default: 20,
            min: 0,
            max: 200,
        },
        false,
        CARDS,
        "",
    ),
    (
        "retention",
        Kind::Int {
            default: 90,
            min: 70,
            max: 97,
        },
        true,
        CARDS,
        "vocabulary.unit.percent",
    ),
    ("cards-from-lookups", Kind::Bool(false), false, CARDS, ""),
    ("review-on-wake", Kind::Bool(false), false, CARDS, ""),
    ("sd-auto", Kind::Bool(true), false, CARDS, ""),
    ("sd-folder", Kind::Text(DEFAULT_FOLDER), false, CARDS, ""),
];

/// The titles and helps are `vocabulary.setting.<name>` and `….help`; they are
/// written out because the builders borrow them.
const TEXTS: [(&str, &str); 8] = [
    (
        "vocabulary.setting.collect",
        "vocabulary.setting.collect.help",
    ),
    ("vocabulary.setting.mark", "vocabulary.setting.mark.help"),
    (
        "vocabulary.setting.new-per-day",
        "vocabulary.setting.new-per-day.help",
    ),
    (
        "vocabulary.setting.retention",
        "vocabulary.setting.retention.help",
    ),
    (
        "vocabulary.setting.cards-from-lookups",
        "vocabulary.setting.cards-from-lookups.help",
    ),
    (
        "vocabulary.setting.review-on-wake",
        "vocabulary.setting.review-on-wake.help",
    ),
    (
        "vocabulary.setting.sd-auto",
        "vocabulary.setting.sd-auto.help",
    ),
    (
        "vocabulary.setting.sd-folder",
        "vocabulary.setting.sd-folder.help",
    ),
];

/// Keeping the words, marking them, and how the flash cards are reviewed.
pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    for ((name, kind, advanced, section, unit), (title, help)) in ROWS.into_iter().zip(TEXTS) {
        let mut spec = SpecDecl::new(name, kind, GROUP, title);
        spec.help = help;
        spec.advanced = advanced;
        declarations.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.section = section;
        extra.unit = unit;
        declarations.extra(extra)?;
    }
    declarations.submit()
}

/// What makes the plugin put its files right: the start (a list or a deck left
/// half written by a power cut).
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.subscribe_event(EventKind::Started, EVENTS_TOKEN)?;
    // A card answered *Again* comes again in the review once its time has come.
    declarations.subscribe_event(EventKind::Tick, TICK_TOKEN)?;
    declarations.submit()
}

/// *Vocabulary* (the list of words) and *Flash cards* in the Tools menu (order
/// 36, as on interface 6), their icons, and the screens: the list, the decks,
/// and the review (one for each deck named).
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.screen(ScreenDecl::new(
        "cards",
        "vocabulary.cards.title",
        DECKS_TOKEN,
    ))?;
    declarations.screen(ScreenDecl::new(
        "review",
        "vocabulary.review.title",
        REVIEW_TOKEN,
    ))?;
    declarations.screen(ScreenDecl::new("list", "vocabulary.title", WORDS_TOKEN))?;
    declarations.action(ActionDecl::new("open", "vocabulary.title", OPEN_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "vocabulary.open");
    entry.order = 36;
    declarations.menu_entry(entry)?;
    declarations.icon("vocabulary.open", "vocabulary")?;
    declarations.action(ActionDecl::new(
        "flash-cards",
        "vocabulary.cards.title",
        CARDS_TOKEN,
    ))?;
    let mut entry = MenuEntryDecl::new("core.tools", "vocabulary.flash-cards");
    entry.order = 36;
    declarations.menu_entry(entry)?;
    declarations.icon("vocabulary.flash-cards", "stack-2")?;
    declarations.submit()
}
