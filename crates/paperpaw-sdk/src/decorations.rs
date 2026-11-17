//! Decorations of text: marks any plugin puts on a book's words, drawn
//! the same by every reader (ADR 0021).
//!
//! - A [`Decoration`] says how text is marked: a background, the text's
//!   ink, a line under it (solid, dotted, dashed, wavy, double) in a tone
//!   and a thickness, a frame, struck through. Tones ([`Tone`]) are named;
//!   panels show them as well as they can (colours where there is colour,
//!   their lightness elsewhere, backgrounds dithered where there are few
//!   grays). `draw_behind` and `draw_over` draw one around the text.
//! - A *role* names what a mark means (`vocabulary.learning`); its look is
//!   settings, one per aspect (see [`add_role`]), in the group
//!   [`groups::MARKS`]: the plugin gives
//!   the default, themes and users the rest. `Decoration::of_role` reads
//!   it.
//! - A *decorator* ([`DecoratorSpec`], `Items::add_decorator`) gets the
//!   words of a page ([`PageText`]) and gives back ranges with their roles
//!   ([`Decorated`]). Readers ask the decorators ([`Decorators`], in
//!   `Env::decorators`) once a page is laid out, keep the answer with the
//!   page, and ask again when a decorator's [`signature`](Decorators::signature)
//!   changes. Where marks meet on a word, their decorations are
//!   [`combined`](Decoration::combined), the earlier decorator winning an
//!   aspect both set.
//! - A decorator that [takes taps](DecoratorSpec::taps) hears
//!   [`events::DECORATION_TAPPED`](crate::builtin::events::DECORATION_TAPPED)
//!   when one of its words is tapped, or chosen with the buttons.
//!
//! What changes widths (bold, a larger size) is a text filter instead
//! (see [`filters`](crate::filters)): it needs the page laid out again.

use alloc::borrow::Cow;
use alloc::format;
use alloc::vec::Vec;
use core::fmt;

use crate::builtin::groups;
use crate::document::Locator;
use crate::{
    Choices, Color, Env, Id, RegisterError, SettingExtra, SettingKind, SettingSpec, SettingValue,
};

/// A named tone for backgrounds, ink and lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Tone {
    LightGray,
    Gray,
    DarkGray,
    Black,
    White,
    Yellow,
    Green,
    Blue,
    Pink,
    Orange,
    Red,
    /// A value a later build adds, by its name (a constant of that build);
    /// a build that doesn't know it: it draws in the default tone.
    Other(&'static str),
}

impl Tone {
    /// Every tone, in the order settings offer them.
    pub const ALL: &[Tone] = &[
        Tone::LightGray,
        Tone::Gray,
        Tone::DarkGray,
        Tone::Black,
        Tone::White,
        Tone::Yellow,
        Tone::Green,
        Tone::Blue,
        Tone::Pink,
        Tone::Orange,
        Tone::Red,
    ];

    /// The colour drawn: as it is on colour panels, by its lightness on
    /// the others (a yellow is a light gray there).
    pub fn color(self) -> Color {
        match self {
            Tone::LightGray => Color::gray(210),
            Tone::Gray => Color::gray(150),
            Tone::DarkGray => Color::gray(90),
            Tone::Black => Color::BLACK,
            Tone::White => Color::WHITE,
            Tone::Yellow => Color::rgb(255, 230, 80),
            Tone::Green => Color::rgb(120, 200, 120),
            Tone::Blue => Color::rgb(90, 160, 240),
            Tone::Pink => Color::rgb(240, 120, 170),
            Tone::Orange => Color::rgb(255, 170, 60),
            Tone::Red => Color::rgb(220, 50, 50),
            // A tone of a later build: drawn as gray, which shows on both
            // light and dark.
            Tone::Other(_) => Tone::Gray.color(),
        }
    }

    /// Its value in settings (also the translation key of its name).
    pub const fn id(self) -> &'static str {
        match self {
            Tone::LightGray => "core.tone.light-gray",
            Tone::Gray => "core.tone.gray",
            Tone::DarkGray => "core.tone.dark-gray",
            Tone::Black => "core.tone.black",
            Tone::White => "core.tone.white",
            Tone::Yellow => "core.tone.yellow",
            Tone::Green => "core.tone.green",
            Tone::Blue => "core.tone.blue",
            Tone::Pink => "core.tone.pink",
            Tone::Orange => "core.tone.orange",
            Tone::Red => "core.tone.red",
            Tone::Other(id) => id,
        }
    }

    /// The tone of a setting's value.
    pub fn from_id(id: &str) -> Option<Tone> {
        // By the names, not the tones: a loop over values of two words
        // each is unrolled into every caller's plugin.
        let at = TONES[1..].iter().position(|name| *name == id)?;
        Tone::ALL.get(at).copied()
    }
}

/// The kind of a line under text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LineKind {
    Solid,
    Dotted,
    Dashed,
    Wavy,
    Double,
    /// A value a later build adds, by its name (a constant of that build);
    /// a build that doesn't know it: it draws the default line.
    Other(&'static str),
}

impl LineKind {
    /// Every kind, in the order settings offer them.
    pub const ALL: &[LineKind] = &[
        LineKind::Solid,
        LineKind::Dotted,
        LineKind::Dashed,
        LineKind::Wavy,
        LineKind::Double,
    ];

    /// Its value in settings (also the translation key of its name).
    pub const fn id(self) -> &'static str {
        match self {
            LineKind::Solid => "core.line.solid",
            LineKind::Dotted => "core.line.dotted",
            LineKind::Dashed => "core.line.dashed",
            LineKind::Wavy => "core.line.wavy",
            LineKind::Double => "core.line.double",
            LineKind::Other(id) => id,
        }
    }

    pub fn from_id(id: &str) -> Option<LineKind> {
        LineKind::ALL.iter().copied().find(|kind| kind.id() == id)
    }
}

/// The value of settings for no background, no line.
pub const NONE: &str = "core.tone.none";
/// The value of the ink setting for the text as it is.
pub const AS_IS: &str = "core.tone.as-is";
/// The thickness of lines and frames where none is set, in tenths of a
/// millimetre.
pub const THICKNESS: u8 = 4;

/// How text is marked (see the [module documentation](self)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct Decoration {
    /// Behind the text.
    pub background: Option<Tone>,
    /// The text itself; `None` as it is.
    pub ink: Option<Tone>,
    /// A line under the text.
    pub line: Option<LineKind>,
    /// The line's (and the frame's) tone; `None` the ink's, else black.
    pub line_tone: Option<Tone>,
    /// Of lines and frames, in tenths of a millimetre; 0 for
    /// [`THICKNESS`].
    pub thickness: u8,
    /// A frame around the text.
    pub frame: bool,
    /// A line through the text.
    pub struck: bool,
}

impl Decoration {
    /// Whether anything is drawn over the text (lines, a frame).
    fn has_strokes(&self) -> bool {
        self.line.is_some() || self.frame || self.struck
    }

    /// The colour of the text, where it is not as it is.
    pub fn ink_color(&self) -> Option<Color> {
        self.ink.map(Tone::color)
    }

    /// This one with what `later` adds: each aspect this one leaves open
    /// taken from `later`; frames and strikes from either; the thickness
    /// goes with the strokes it came with.
    pub fn combined(self, later: Decoration) -> Decoration {
        Decoration {
            background: self.background.or(later.background),
            ink: self.ink.or(later.ink),
            line: self.line.or(later.line),
            line_tone: self.line_tone.or(later.line_tone),
            thickness: if self.has_strokes() || !later.has_strokes() {
                self.thickness
            } else {
                later.thickness
            },
            frame: self.frame || later.frame,
            struck: self.struck || later.struck,
        }
    }

    /// No mark at all.
    pub const NONE: Decoration = Decoration {
        background: None,
        ink: None,
        line: None,
        line_tone: None,
        thickness: 0,
        frame: false,
        struck: false,
    };

    /// This one with `tone` behind the text.
    pub const fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
    }

    /// This one with the text in `tone`.
    pub const fn ink(mut self, tone: Tone) -> Self {
        self.ink = Some(tone);
        self
    }

    /// This one with a line of `kind` under the text.
    pub const fn line(mut self, kind: LineKind) -> Self {
        self.line = Some(kind);
        self
    }

    /// This one with lines and frames in `tone`.
    pub const fn line_tone(mut self, tone: Tone) -> Self {
        self.line_tone = Some(tone);
        self
    }

    /// This one with lines and frames `tenths` of a millimetre thick.
    pub const fn thickness(mut self, tenths: u8) -> Self {
        self.thickness = tenths;
        self
    }

    /// This one with a frame.
    pub const fn frame(mut self) -> Self {
        self.frame = true;
        self
    }

    /// This one struck through.
    pub const fn struck(mut self) -> Self {
        self.struck = true;
        self
    }

    /// Whether it marks nothing.
    pub fn is_none(&self) -> bool {
        self.background.is_none()
            && self.ink.is_none()
            && self.line.is_none()
            && !self.frame
            && !self.struck
    }
}

/// The options of a tone setting: `none` (no tone, or the text as it
/// is), then every tone.
const fn tone_options(none: &'static str) -> [&'static str; Tone::ALL.len() + 1] {
    let mut options = [none; Tone::ALL.len() + 1];
    let mut i = 0;
    while i < Tone::ALL.len() {
        options[i + 1] = Tone::ALL[i].id();
        i += 1;
    }
    options
}

/// The options of a role's background, and of its ink and line tone.
static TONES: [&str; Tone::ALL.len() + 1] = tone_options(NONE);
static INKS: [&str; Tone::ALL.len() + 1] = tone_options(AS_IS);

/// The options of a role's line: none, then every kind.
static LINES: [&str; LineKind::ALL.len() + 1] = {
    let mut options = [NONE; LineKind::ALL.len() + 1];
    let mut i = 0;
    while i < LineKind::ALL.len() {
        options[i + 1] = LineKind::ALL[i].id();
        i += 1;
    }
    options
};

const fn tone_value(tone: Option<Tone>, none: &'static str) -> SettingValue {
    SettingValue::Text(Cow::Borrowed(match tone {
        Some(tone) => tone.id(),
        None => none,
    }))
}

const fn line_value(line: Option<LineKind>) -> SettingValue {
    SettingValue::Text(Cow::Borrowed(match line {
        Some(kind) => kind.id(),
        None => NONE,
    }))
}

const fn default_thickness(default: Decoration) -> i64 {
    if default.thickness == 0 {
        THICKNESS as i64
    } else {
        default.thickness as i64
    }
}

/// The aspects of a role, each a setting `<role>.<aspect>`.
const ASPECTS: [&str; 7] = [
    "background",
    "ink",
    "line",
    "line-tone",
    "thickness",
    "frame",
    "struck",
];

/// Adds role `role` through `add` (the kernel's `Items::add_setting`):
/// its settings, one per aspect, in the group
/// [`groups::MARKS`] under a section `title` (the role's name, a
/// translation key), with `default` as their defaults. The settings are
/// `<role>.background`, `.ink`, `.line`, `.line-tone`, `.thickness`,
/// `.frame` and `.struck`; themes set their defaults like any setting's.
/// A role known when the plugin is written takes less memory with
/// [`role_settings!`](crate::role_settings) instead.
pub fn add_role(
    add: &mut dyn FnMut(SettingSpec) -> Result<(), RegisterError>,
    role: &Id,
    title: impl Into<Cow<'static, str>>,
    default: Decoration,
) -> Result<(), RegisterError> {
    let title: Cow<'static, str> = title.into();
    let id = |aspect: &str| {
        Id::parse(&format!("{role}.{aspect}")).map_err(|_| RegisterError::Invalid {
            message: Cow::Borrowed("a role's id makes no setting ids"),
        })
    };
    let choice = |aspect, title, options: &'static [&'static str], value| {
        Ok::<_, RegisterError>(SettingSpec::new(
            id(aspect)?,
            title,
            SettingKind::Choice(Choices::Fixed(options)),
            value,
            crate::builtin::groups::OTHER,
        ))
    };
    let specs = [
        choice(
            ASPECTS[0],
            "core.mark.background",
            &TONES,
            tone_value(default.background, NONE),
        )?,
        choice(
            ASPECTS[1],
            "core.mark.ink",
            &INKS,
            tone_value(default.ink, AS_IS),
        )?,
        choice(
            ASPECTS[2],
            "core.mark.line",
            &LINES,
            line_value(default.line),
        )?,
        choice(
            ASPECTS[3],
            "core.mark.line-tone",
            &INKS,
            tone_value(default.line_tone, AS_IS),
        )?,
        SettingSpec::new(
            id(ASPECTS[4])?,
            "core.mark.thickness",
            SettingKind::Int { min: 1, max: 20 },
            SettingValue::Int(default_thickness(default)),
            crate::builtin::groups::OTHER,
        ),
        SettingSpec::new(
            id(ASPECTS[5])?,
            "core.mark.frame",
            SettingKind::Bool,
            SettingValue::Bool(default.frame),
            crate::builtin::groups::OTHER,
        ),
        SettingSpec::new(
            id(ASPECTS[6])?,
            "core.mark.struck",
            SettingKind::Bool,
            SettingValue::Bool(default.struck),
            crate::builtin::groups::OTHER,
        ),
    ];
    for (mut spec, aspect) in specs.into_iter().zip(ASPECTS) {
        // A role from data: its section, its name, is known only now.
        let mut extra = SettingExtra::new();
        extra.section = Some(title.clone());
        if aspect == "thickness" {
            extra.unit = Some(Cow::Borrowed("core.unit.tenth-mm"));
        }
        spec.group = groups::MARKS;
        spec.description = Some(crate::Str::new(format!("core.mark.{aspect}.help")));
        add(spec.with_owned_extra(extra))?;
    }
    Ok(())
}

/// The settings of a role (see [`add_role`]) kept in the program, for
/// `Items::add_settings`: made by [`role_settings!`](crate::role_settings).
#[doc(hidden)]
pub const fn fixed_role(
    ids: [&'static str; 7],
    section: &'static SettingExtra,
    thickness: &'static SettingExtra,
    default: Decoration,
) -> [SettingSpec; 7] {
    const fn choice(
        id: &'static str,
        title: &'static str,
        kind: SettingKind,
        default: SettingValue,
        group: Id,
    ) -> SettingSpec {
        SettingSpec::new(Id::new_static(id), title, kind, default, group)
    }
    let tones = SettingKind::Choice(Choices::Fixed(&TONES));
    let inks = SettingKind::Choice(Choices::Fixed(&INKS));
    let lines = SettingKind::Choice(Choices::Fixed(&LINES));
    [
        choice(
            ids[0],
            "core.mark.background",
            tones,
            tone_value(default.background, NONE),
            groups::MARKS,
        )
        .with_extra(section)
        .description("core.mark.background.help"),
        choice(
            ids[1],
            "core.mark.ink",
            inks,
            tone_value(default.ink, AS_IS),
            groups::MARKS,
        )
        .with_extra(section)
        .description("core.mark.ink.help"),
        choice(
            ids[2],
            "core.mark.line",
            lines,
            line_value(default.line),
            groups::MARKS,
        )
        .with_extra(section)
        .description("core.mark.line.help"),
        choice(
            ids[3],
            "core.mark.line-tone",
            SettingKind::Choice(Choices::Fixed(&INKS)),
            tone_value(default.line_tone, AS_IS),
            groups::MARKS,
        )
        .with_extra(section)
        .description("core.mark.line-tone.help"),
        SettingSpec::number(
            ids[4],
            "core.mark.thickness",
            1,
            20,
            default_thickness(default),
            groups::MARKS,
        )
        .with_extra(thickness)
        .description("core.mark.thickness.help"),
        SettingSpec::switch(ids[5], "core.mark.frame", default.frame, groups::MARKS)
            .with_extra(section)
            .description("core.mark.frame.help"),
        SettingSpec::switch(ids[6], "core.mark.struck", default.struck, groups::MARKS)
            .with_extra(section)
            .description("core.mark.struck.help"),
    ]
}

/// The settings of a role, as [`add_role`] adds them, kept in the
/// program: a `static` list for `Items::add_settings`, which takes
/// next to no memory. The role and its title (a translation key) are
/// written out:
///
/// ```
/// use paperpaw_sdk::SettingSpec;
/// use paperpaw_sdk::decorations::{Decoration, LineKind};
/// static LEARNING: [SettingSpec; 7] = paperpaw_sdk::role_settings!(
///     "my.learning",
///     "my.role.learning",
///     Decoration::NONE.line(LineKind::Dotted)
/// );
/// // In `register`: r.add_settings(&LEARNING)?;
/// ```
#[macro_export]
macro_rules! role_settings {
    ($role:literal, $title:literal, $default:expr $(,)?) => {{
        static SECTION: $crate::SettingExtra = $crate::SettingExtra::new().section($title);
        static THICKNESS: $crate::SettingExtra = $crate::SettingExtra::new()
            .section($title)
            .unit("core.unit.tenth-mm");
        $crate::decorations::fixed_role(
            [
                concat!($role, ".background"),
                concat!($role, ".ink"),
                concat!($role, ".line"),
                concat!($role, ".line-tone"),
                concat!($role, ".thickness"),
                concat!($role, ".frame"),
                concat!($role, ".struck"),
            ],
            &SECTION,
            &THICKNESS,
            $default,
        )
    }};
}

/// A word of a page given to decorators: its text and where it is in the
/// book. A word split over two lines (hyphenated) is two page words, each
/// with the whole word as its text and its own part's place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageWord<'a> {
    pub text: &'a str,
    pub start: Locator,
    /// Just after its last character.
    pub end: Locator,
}

/// The words of a page, for decorators.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct PageText<'a> {
    /// The book.
    pub path: &'a str,
    /// Its language, e.g. `de` (empty where unknown).
    pub language: &'a str,
    /// In reading order.
    pub words: &'a [PageWord<'a>],
}

/// A range a decorator marks, with its role.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Decorated {
    pub start: Locator,
    /// Just after its last character.
    pub end: Locator,
    pub role: Id,
}

impl Decorated {
    /// The range of `words` (from the first's start to the last's end) in
    /// `role`.
    pub(crate) fn words(words: &[PageWord<'_>], role: &Id) -> Option<Decorated> {
        Some(Decorated {
            start: words.first()?.start,
            end: words.last()?.end,
            role: role.clone(),
        })
    }
}

/// What a decorator does: a page's words in, the ranges it marks out.
pub(crate) type DecorateFn = fn(&Env<'_>, &PageText<'_>) -> Vec<Decorated>;
/// What a decorator's marks depend on besides the page (its data, its
/// settings): readers ask again when it changes.
pub(crate) type DecoratorSignatureFn = fn(&Env<'_>) -> u64;

/// How a decorator marks a page: by a function, or by the rules of a
/// [`PatternDecorator`] (a plugin without code's `marks.ini`).
#[non_exhaustive]
pub enum Decorate {
    Fn(DecorateFn),
    Patterns(PatternDecorator),
}

impl core::fmt::Debug for Decorate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Decorate::Fn(_) => f.write_str("Fn"),
            Decorate::Patterns(patterns) => f.debug_tuple("Patterns").field(patterns).finish(),
        }
    }
}

/// A decorator contribution (see the [module documentation](self)).
#[non_exhaustive]
pub struct DecoratorSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// Decorators run by order, smallest first; the earlier wins an
    /// aspect two set on the same word.
    pub order: i16,
    /// A setting that switches it on and off, if any.
    pub switch: Option<Id>,
    /// It hears taps on what it marks
    /// ([`events::DECORATION_TAPPED`](crate::builtin::events::DECORATION_TAPPED)).
    pub taps: bool,
    pub signature: DecoratorSignatureFn,
    pub decorate: Decorate,
}

impl DecoratorSpec {
    /// Decorator `id`, named `title`, marking with `decorate`; order 0,
    /// always on, no taps, no signature.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, decorate: DecorateFn) -> Self {
        Self::by(id, title, Decorate::Fn(decorate))
    }

    fn by(id: Id, title: impl Into<Cow<'static, str>>, decorate: Decorate) -> Self {
        Self {
            id,
            title: title.into(),
            order: 0,
            switch: None,
            taps: false,
            signature: |_| 0,
            decorate,
        }
    }

    pub fn order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }

    /// What its marks depend on besides the page. Readers ask for it
    /// whenever they draw a page (on some devices once for every band of
    /// it): it says it from memory, e.g. a count of the changes to its
    /// data, without reading the storage.
    pub fn signature(mut self, signature: DecoratorSignatureFn) -> Self {
        self.signature = signature;
        self
    }
}

/// Marks words by [patterns](crate::pattern): each found on the page's
/// text (its words with a space between), the words it touches marked in
/// its role. A word broken over two lines counts once, whole.
#[derive(Debug, Clone, Default)]
pub struct PatternDecorator {
    rules: Vec<(crate::pattern::Pattern, Id)>,
}

impl PatternDecorator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks what `pattern` finds in `role`; rules are tried in order.
    pub(crate) fn rule(mut self, pattern: crate::pattern::Pattern, role: Id) -> Self {
        self.rules.push((pattern, role));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// A decorator `id`, named `title`, marking by these rules.
    pub fn into_spec(self, id: Id, title: impl Into<Cow<'static, str>>) -> DecoratorSpec {
        DecoratorSpec::by(id, title, Decorate::Patterns(self))
    }
}

/// The marks a plugin without code describes in its `marks.ini`: a
/// section each, which becomes the role `<plugin>.<section>` (its look
/// settings under its title in Settings, Marks in the text) and rules of
/// a [`PatternDecorator`]:
///
/// ```ini
/// [names]
/// title = Names                 ; a translation key, or the text itself
/// pattern = \b(?:Mr|Mrs|Miss)\. [A-Z]\w+
/// pattern = \bthe White Rabbit\b
/// background = yellow           ; light-gray, gray, dark-gray, black, white,
/// ink = as-is                   ; yellow, green, blue, pink, orange, red,
/// line = wavy                   ; none, or as-is (ink); solid, dotted,
/// line-tone = red               ; dashed, wavy, double
/// thickness = 6                 ; in tenths of a millimetre, 1 to 20
/// frame = off
/// struck = off
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct MarkRules {
    /// The roles: their ids, titles and default looks.
    pub roles: Vec<(Id, alloc::string::String, Decoration)>,
    /// Their patterns.
    pub decorator: PatternDecorator,
}

impl MarkRules {
    /// The marks of plugin `plugin` in `source` (a `marks.ini`).
    pub fn parse(plugin: &Id, source: &str) -> Result<MarkRules, RegisterError> {
        use alloc::string::ToString;
        let mut rules = MarkRules::default();
        for entry in paperpaw_config::parse(source) {
            let entry = entry.map_err(|error| RegisterError::Parse {
                line: error.line,
                message: Cow::Owned(error.to_string()),
            })?;
            let line = entry.line;
            let error = |message: alloc::string::String| RegisterError::Parse {
                line,
                message: Cow::Owned(message),
            };
            let value = entry.value.as_str();
            let role = Id::parse(&format!("{plugin}.{}", entry.section))
                .map_err(|_| error(format!("`[{}]` names no role", entry.section)))?;
            if rules.roles.last().is_none_or(|(id, ..)| *id != role) {
                if rules.roles.iter().any(|(id, ..)| *id == role) {
                    return Err(error(format!("`[{}]` twice", entry.section)));
                }
                rules
                    .roles
                    .push((role.clone(), entry.section.to_string(), Decoration::NONE));
            }
            let Some((_, title, look)) = rules.roles.last_mut() else {
                continue;
            };
            let tone = |value: &str| {
                let id = format!("core.tone.{value}");
                Tone::from_id(&id).ok_or_else(|| error(format!("no tone `{value}`")))
            };
            let switch = || {
                entry
                    .value
                    .as_bool()
                    .ok_or_else(|| error(format!("`{}` is on or off", entry.key)))
            };
            match (entry.key, &*value) {
                ("title", _) => *title = value.to_string(),
                ("pattern", _) => {
                    let pattern = crate::pattern::Pattern::new(&value)
                        .map_err(|e| error(format!("pattern: {e}")))?;
                    rules.decorator = core::mem::take(&mut rules.decorator).rule(pattern, role);
                }
                ("background" | "ink" | "line-tone", "none" | "as-is") => {}
                ("background", _) => look.background = Some(tone(&value)?),
                ("ink", _) => look.ink = Some(tone(&value)?),
                ("line-tone", _) => look.line_tone = Some(tone(&value)?),
                ("line", "none") => look.line = None,
                ("line", _) => {
                    let kind = LineKind::from_id(&format!("core.line.{value}"))
                        .ok_or_else(|| error(format!("no line `{value}`")))?;
                    look.line = Some(kind);
                }
                ("thickness", _) => {
                    let tenths = entry
                        .value
                        .as_u32()
                        .filter(|t| (1..=20).contains(t))
                        .ok_or_else(|| error("thickness is 1 to 20".to_string()))?;
                    look.thickness = tenths as u8;
                }
                ("frame", _) => look.frame = switch()?,
                ("struck", _) => look.struck = switch()?,
                (key, _) => return Err(error(format!("no `{key}` in a mark"))),
            }
        }
        Ok(rules)
    }
}

impl fmt::Debug for DecoratorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecoratorSpec")
            .field("id", &self.id)
            .field("order", &self.order)
            .field("taps", &self.taps)
            .finish_non_exhaustive()
    }
}

/// The registered decorators, by order.
#[derive(Debug, Default)]
pub struct Decorators {
    list: Vec<DecoratorSpec>,
}

impl Decorators {
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `decorator` after those of a smaller or the same order.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, decorator: DecoratorSpec) {
        let at = self
            .list
            .iter()
            .position(|d| d.order > decorator.order)
            .unwrap_or(self.list.len());
        self.list.insert(at, decorator);
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The decorator `id`, e.g. for its name.
    pub fn get(&self, id: &Id) -> Option<&DecoratorSpec> {
        self.list.iter().find(|d| d.id == *id)
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.list.shrink_to_fit();
    }

    /// What the marks of all decorators depend on besides the page: which
    /// are on and their signatures. Readers ask them again when it
    /// changes.
    pub fn signature(&self, env: &Env<'_>) -> u64 {
        // FNV-1a as `paperpaw_digest::fnv1a64` computes it, fed piece by
        // piece: readers ask on every band, so nothing is allocated.
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let feed = |hash: u64, bytes: &[u8]| {
            bytes
                .iter()
                .fold(hash, |h, &b| (h ^ u64::from(b)).wrapping_mul(PRIME))
        };
        self.list.iter().fold(OFFSET, |hash, decorator| {
            let on = decorator
                .switch
                .as_ref()
                .is_none_or(|s| env.settings().bool(s.as_str()));
            let hash = feed(hash, decorator.id.as_str().as_bytes());
            let hash = feed(hash, &[u8::from(on)]);
            if on {
                feed(hash, &(decorator.signature)(env).to_le_bytes())
            } else {
                hash
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_and_lines_of_a_later_build_keep_their_name() {
        let purple = Tone::Other("core.tone.purple");
        assert_eq!(
            (purple.id(), purple.color()),
            ("core.tone.purple", Tone::Gray.color())
        );
        assert_eq!(LineKind::Other("core.line.zigzag").id(), "core.line.zigzag");
        // A name this build doesn't know is not read as one of its tones.
        assert_eq!(Tone::from_id("core.tone.purple"), None);
    }

    #[test]
    fn tones_and_lines_by_their_ids() {
        for &tone in Tone::ALL {
            assert_eq!(Tone::from_id(tone.id()), Some(tone));
        }
        for &kind in LineKind::ALL {
            assert_eq!(LineKind::from_id(kind.id()), Some(kind));
        }
        assert_eq!(Tone::from_id(NONE), None);
        assert_eq!(Tone::from_id(AS_IS), None);
    }
}
