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
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, HelpHintDecl,
    Instances, Kind, MenuEntryDecl, ScreenDecl, SlotOfferDecl, SpecDecl, SubscribeDecl, WidgetDecl,
};
use paperpaw_sdk::native::ui::HintInput;
use paperpaw_ui_kit::widget::options::{Opt, declare_all};

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
/// The card files' menu, and the screen that updates from them or saves a deck.
pub const SD_MENU_TOKEN: u32 = 13;
pub const SD_CARD_TOKEN: u32 = 14;
/// The work that does the next step of an update or a save.
pub const WORK_TOKEN: u32 = 5;
/// The stage of the words looked up, and its token.
pub const WORD_TOKEN: u32 = 6;
pub const WORD_STAGE: &str = "reader.word-looked-up";
/// The stage the reader asks for marks, and its token.
pub const DECORATE_TOKEN: u32 = 8;
pub const DECORATE_STAGE: &str = "reader.decorations";

/// The sleep card's screen, and the waking (the review may open).
pub const SLEEP_TOKEN: u32 = 15;
pub const RESUME_TOKEN: u32 = 7;
/// The sleep card's screen id and its place among the choices of `sleep.show`.
pub const SLEEP_CARD: &str = "vocabulary.sleep-card";

/// The widgets' tokens: the word of the day, the words saved, the cards due.
pub const WORD_WIDGET_TOKEN: u32 = 20;
pub const SAVED_WIDGET_TOKEN: u32 = 21;
pub const DUE_WIDGET_TOKEN: u32 = 22;

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

/// The tones a mark can have (the SDK's `Tone::ALL` ids), the background's and
/// the ink's choices add `core.tone.none` and `core.tone.as-is` before them.
const TONE_IDS: [&str; 11] = [
    "core.tone.light-gray",
    "core.tone.gray",
    "core.tone.dark-gray",
    "core.tone.black",
    "core.tone.white",
    "core.tone.yellow",
    "core.tone.green",
    "core.tone.blue",
    "core.tone.pink",
    "core.tone.orange",
    "core.tone.red",
];
const BACKGROUNDS: [&str; 12] = tones("core.tone.none");
const INKS: [&str; 12] = tones("core.tone.as-is");
const LINES: [&str; 6] = [
    "core.tone.none",
    "core.line.solid",
    "core.line.dotted",
    "core.line.dashed",
    "core.line.wavy",
    "core.line.double",
];

/// `none`, then every tone.
const fn tones(none: &'static str) -> [&'static str; 12] {
    let mut all = [none; 12];
    let mut at = 0;
    while at < TONE_IDS.len() {
        all[at + 1] = TONE_IDS[at];
        at += 1;
    }
    all
}

/// The role of the words being learnt, marked in books: how they look, as the
/// SDK's `role_settings!` made it (`vocabulary.learning.<aspect>`, section
/// `vocabulary.role.learning`, the marks' group), with Beta 3's defaults: a dotted
/// line, four tenths of a millimetre thick.
pub const ROLE_ASPECTS: [&str; 7] = [
    "background",
    "ink",
    "line",
    "line-tone",
    "thickness",
    "frame",
    "struck",
];

/// Declares the role's seven settings.
fn role_settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    declarations: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    const TITLES: [&str; 7] = [
        "core.mark.background",
        "core.mark.ink",
        "core.mark.line",
        "core.mark.line-tone",
        "core.mark.thickness",
        "core.mark.frame",
        "core.mark.struck",
    ];
    const HELPS: [&str; 7] = [
        "core.mark.background.help",
        "core.mark.ink.help",
        "core.mark.line.help",
        "core.mark.line-tone.help",
        "core.mark.thickness.help",
        "core.mark.frame.help",
        "core.mark.struck.help",
    ];
    const NAMES: [&str; 7] = [
        "learning.background",
        "learning.ink",
        "learning.line",
        "learning.line-tone",
        "learning.thickness",
        "learning.frame",
        "learning.struck",
    ];
    let kinds: [Kind<'static>; 7] = [
        Kind::Choice {
            default: "core.tone.none",
            choices: &BACKGROUNDS,
        },
        Kind::Choice {
            default: "core.tone.as-is",
            choices: &INKS,
        },
        Kind::Choice {
            default: "core.line.dotted",
            choices: &LINES,
        },
        Kind::Choice {
            default: "core.tone.as-is",
            choices: &INKS,
        },
        Kind::Int {
            default: 4,
            min: 1,
            max: 20,
        },
        Kind::Bool(false),
        Kind::Bool(false),
    ];
    for at in 0..NAMES.len() {
        let mut spec = SpecDecl::new(NAMES[at], kinds[at].clone(), MARKS_GROUP, TITLES[at]);
        spec.help = HELPS[at];
        declarations.spec(spec)?;
        let mut extra = ExtraDecl::new(NAMES[at]);
        extra.section = "vocabulary.role.learning";
        if ROLE_ASPECTS[at] == "thickness" {
            extra.unit = "core.unit.tenth-mm";
        }
        declarations.extra(extra)?;
    }
    Ok(())
}

/// The group the marks' looks are in (`core.group.marks`).
pub const MARKS_GROUP: &str = "core.group.marks";

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
    role_settings(&mut declarations)?;
    declarations.submit()
}

/// What makes the plugin put its files right: the start (a list or a deck left
/// half written by a power cut).
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.subscribe_event(EventKind::Started, EVENTS_TOKEN)?;
    // A card answered *Again* comes again in the review once its time has come.
    declarations.subscribe_event(EventKind::Tick, TICK_TOKEN)?;
    // The next step of an update from the card, or of a save to it.
    declarations.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    // Waking: the review opens with the card shown asleep, or where wanted.
    declarations.subscribe_event(EventKind::Resume, RESUME_TOKEN)?;
    declarations.submit()
}

/// The stage the reader runs when a word is looked up
/// (`reader.word-looked-up`, schema 1: `stages/subscribed/`): this plugin keeps
/// the word, as the interface-6 event `core.word-looked-up` did.
pub fn stages(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.subscribe(SubscribeDecl::new(WORD_STAGE, WORD_TOKEN))?;
    // The words being learnt, marked in books, and a tap on one
    // (`reader.decorations`, schema 1: `stages/decorations/`).
    declarations.subscribe(SubscribeDecl::new(DECORATE_STAGE, DECORATE_TOKEN))?;
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
    declarations.screen(
        ScreenDecl::new("sleep-card", SLEEP_CARD, SLEEP_TOKEN).instances(Instances::Single),
    )?;
    declarations.screen(ScreenDecl::new(
        "sd-menu",
        "vocabulary.sd.title",
        SD_MENU_TOKEN,
    ))?;
    declarations.screen(ScreenDecl::new(
        "sd-card",
        "vocabulary.sd.title",
        SD_CARD_TOKEN,
    ))?;
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

/// Which word the word of the day is: another each day, or the newest.
pub const PICKS: [&str; 2] = ["vocabulary.pick.daily", "vocabulary.pick.newest"];

/// A widget of the plugin.
#[derive(Debug, Clone, Copy)]
pub struct Def {
    /// Relative to the plugin: `widget.word` is `vocabulary.widget.word`.
    pub name: &'static str,
    pub token: u32,
    /// The widget's own settings, in order, after the look they share.
    pub options: &'static [Opt],
}

/// The look the widgets share, in the order interface 6 listed it, without the
/// size and the alignment (which follow, as each widget has them).
const fn look() -> [Opt; 7] {
    [
        Opt::Frame("core.frame.none"),
        Opt::Padding,
        Opt::Paper,
        Opt::Ink,
        Opt::Corners,
        Opt::FrameLine,
        Opt::Shadow,
    ]
}

const WORD_OWN: &[Opt] = &[
    Opt::Size("core.size.medium", true),
    Opt::Align("core.align.left"),
    Opt::Choice(
        "pick",
        "vocabulary.setting.pick",
        &PICKS,
        "vocabulary.pick.daily",
        "vocabulary.setting.pick.help",
    ),
    Opt::Part("caption", "vocabulary.part.caption", true),
    Opt::Part("sentence", "vocabulary.part.sentence", true),
    Opt::Part("book", "vocabulary.part.book", false),
];

const SAVED_OWN: &[Opt] = &[
    Opt::Size("core.size.medium", true),
    Opt::Align("core.align.left"),
    Opt::Part("caption", "vocabulary.part.caption", true),
    Opt::Part("newest", "vocabulary.part.newest", true),
    Opt::Part("books", "vocabulary.part.books", false),
    Opt::Part("languages", "vocabulary.part.languages", false),
];

const DUE_OWN: &[Opt] = &[
    Opt::Size("core.size.medium", true),
    Opt::Align("core.align.left"),
    Opt::Part("caption", "vocabulary.part.caption", true),
];

/// The widgets, with their own settings (the shared look comes first).
pub const WIDGETS: [Def; 3] = [
    Def {
        name: "widget.word",
        token: WORD_WIDGET_TOKEN,
        options: WORD_OWN,
    },
    Def {
        name: "widget.saved",
        token: SAVED_WIDGET_TOKEN,
        options: SAVED_OWN,
    },
    Def {
        name: "widget.due",
        token: DUE_WIDGET_TOKEN,
        options: DUE_OWN,
    },
];

impl Def {
    /// Every setting of the widget in order: the shared look, then its own.
    #[allow(
        dead_code,
        reason = "the tests read it; the build script reads the same file and does not"
    )]
    pub fn all(&self) -> impl Iterator<Item = Opt> + '_ {
        look().into_iter().chain(self.options.iter().copied())
    }
}

/// The widgets with the group of settings each: *Word of the day*, *Words
/// saved* and *Cards due* (the word of the day follows the day, so it is live).
pub fn widgets(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(sink);
    for widget in WIDGETS {
        let group = ["vocabulary.", widget.name].concat();
        d.group(GroupDecl::new(widget.name, &group))?;
        let mut spec = WidgetDecl::new(widget.name, &group, "core.widgets.words", widget.token)
            .map_err(|_| DeclareError::TooLong("widget"))?;
        if widget.token == WORD_WIDGET_TOKEN {
            spec = spec.live();
        }
        // No `previews`: a widget that previews a settings group is never
        // offered in the Add picker (Kernel ruling, #228), and Beta 3 declared
        // none.
        d.widget(spec)?;
        declare_all(&mut d, widget.name, &group, widget.all())?;
        d.submit()?;
    }
    Ok(())
}

/// The sleep card offered to the slot `core.sleep` below the sleep plugin's own
/// (priority 0), and its place among the choices of `sleep.show`: *A flash card
/// due*. The host shows the screen as the sleep screen and replaces it on waking.
pub fn navigation(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(sink);
    let mut offer = SlotOfferDecl::new("core.sleep", SLEEP_CARD);
    offer.priority = -1;
    d.slot_offer(offer)?;
    d.setting_choice("sleep.show", SLEEP_CARD, SLEEP_CARD)?;
    d.submit()
}

/// The help of the review (the first review: the answers, by touch or with the
/// buttons), as interface 6 listed it.
pub fn help(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(sink);
    for (text, input) in [
        ("vocabulary.help.answers", HintInput::Any),
        ("vocabulary.help.tap", HintInput::Touch),
        ("vocabulary.help.select", HintInput::Buttons),
        ("vocabulary.help.back", HintInput::Any),
    ] {
        let hint = HelpHintDecl::new("vocabulary.review", text, input)
            .map_err(|_| DeclareError::Empty("help hint"))?;
        d.help_hint(hint)?;
    }
    d.submit()
}
