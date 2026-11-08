//! Vocabulary's declarations as borrowed builders, shared by the build script
//! (which writes them into the plugin image) and the tests.
//!
//! Part of the port: the eight settings and the start event. The role of the
//! words marked in books (seven more settings), the menu entries, actions,
//! screens, the sleep card and the widget follow with their host pieces (see
//! `PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, ExtraDecl, Kind, SpecDecl,
};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;

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
    declarations.submit()
}
