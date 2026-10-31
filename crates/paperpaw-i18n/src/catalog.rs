use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use paperpaw_config::ParseError;

use crate::plural::PluralRule;

/// The messages of one language.
///
/// Messages are not copied: the catalog keeps its source (usually text
/// built into the program) and an index of where each message's line
/// starts in it, 2 bytes per message (4 in a source of 32 KiB or more);
/// the key and value are read from the
/// line when looked up. Messages the line does not give plainly (quoted
/// values, escape sequences) are kept as byte ranges instead, and only
/// values with escape sequences are stored decoded. A catalog of an
/// unused language can drop its index ([`Catalog::unload`]) and rebuild it
/// when needed.
#[derive(Debug, Clone)]
pub struct Catalog {
    source: Cow<'static, str>,
    language: Span,
    name: Span,
    plural: PluralRule,
    /// `[meta] direction`: 0 not said, 1 `ltr`, 2 `rtl`.
    direction: u8,
    /// `[meta] script`: its place in `paperpaw_lang::scripts()` and 1;
    /// 0 not said.
    script: u8,
    /// `[meta] digits`: its place in [`SYSTEMS`] (0: `latn`), or
    /// [`LITERAL`].
    digits: u8,
    /// `[meta] calendar`: its place in [`CALENDARS`] (0: `gregorian`).
    calendar: u8,
    /// Decoded values that contained escape sequences; the meta values
    /// first (`meta_len` bytes), which stay when the catalog is unloaded.
    /// Boxed, as it changes only when the catalog is scanned: two words
    /// rather than three, in each catalog of every plugin.
    escaped: Box<str>,
    meta_len: u32,
    /// The messages, sorted by key; empty while unloaded. Each is where
    /// its line's key starts in the source or, with [`OTHER`] set, the
    /// place of its key and value in `others`.
    index: Index,
    /// Messages whose line does not give them plainly.
    others: Box<[(Span, Span)]>,
    loaded: bool,
    /// The index holds wide entries, each in two of its places (see
    /// [`Index`]); kept here, beside the other small fields, rather than
    /// in the index, where it would take a word.
    wide: bool,
    /// When it was added to a translator, of those added: later ones win.
    pub(crate) added: u32,
}

/// The numbering systems a catalog can name (`[meta] digits`), as the
/// CLDR names them, each with its digit zero: Unicode keeps a system's ten
/// digits in a row (`hanidec`, which it does not, is [`HANIDEC`]). A
/// catalog's place is its index here: only added at the end (the
/// interface test records them). Any other system is given by its ten
/// digits (`digits = ០១២៣៤៥៦៧៨៩`).
const SYSTEMS: [(&str, u32); 25] = [
    ("latn", 0x30),
    // Arabic-Indic; Extended Arabic-Indic (Persian, Urdu).
    ("arab", 0x660),
    ("arabext", 0x6f0),
    ("deva", 0x966),
    ("beng", 0x9e6),
    ("thai", 0xe50),
    ("fullwide", 0xff10),
    ("hanidec", 0),
    ("khmr", 0x17e0),
    ("laoo", 0xed0),
    ("mymr", 0x1040),
    ("tibt", 0xf20),
    ("guru", 0xa66),
    ("gujr", 0xae6),
    ("orya", 0xb66),
    ("tamldec", 0xbe6),
    ("telu", 0xc66),
    ("knda", 0xce6),
    ("mlym", 0xd66),
    // Sinhala Lith digits.
    ("sinh", 0xde6),
    ("mong", 0x1810),
    ("java", 0xa9d0),
    ("bali", 0x1b50),
    ("adlm", 0x1e950),
    ("nkoo", 0x7c0),
];

/// The Chinese decimal digits (`hanidec`), not in a row.
const HANIDEC: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// Latin digits: where a catalog says none, or none this release knows.
const LATIN: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];

/// A catalog's digits place when `[meta] digits` gives the ten digits
/// themselves: read again from the source when asked.
const LITERAL: u8 = u8::MAX;

/// The calendars a catalog can name (`[meta] calendar`), as the CLDR names
/// them; a catalog's place is its index here: only added at the end. A name
/// is only what a catalog asks for: the dates of a calendar come with what
/// draws them.
const CALENDARS: [&str; 18] = [
    "gregorian",
    // Hijri; Solar Hijri.
    "islamic",
    "persian",
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "hebrew",
    "indian",
    "islamic-civil",
    "islamic-rgsa",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "roc",
];

/// The numbering systems a catalog can name in `[meta] digits`, in the
/// order of their place in a catalog (others are given by their ten
/// digits).
pub fn digit_systems() -> impl Iterator<Item = &'static str> {
    SYSTEMS.iter().map(|(name, _)| *name)
}

/// The calendars a catalog can name in `[meta] calendar`, in the order of
/// their place in a catalog.
pub fn calendars() -> impl Iterator<Item = &'static str> {
    CALENDARS.into_iter()
}

/// The ten digits of the system at `place` in [`SYSTEMS`].
fn system_digits(place: u8) -> Option<[char; 10]> {
    match *SYSTEMS.get(usize::from(place))? {
        ("hanidec", _) => Some(HANIDEC),
        (_, zero) => {
            let mut digits = LATIN;
            for (d, digit) in (0..).zip(digits.iter_mut()) {
                *digit = char::from_u32(zero + d)?;
            }
            Some(digits)
        }
    }
}

/// The ten characters of `value`, if it has exactly ten.
fn ten(value: &str) -> Option<[char; 10]> {
    let mut digits = LATIN;
    let mut chars = value.chars();
    for digit in &mut digits {
        *digit = chars.next()?;
    }
    chars.next().is_none().then_some(digits)
}

/// The place of `[meta] digits = value`: a system's name, [`LITERAL`] for
/// ten digits given, otherwise 0 (Latin).
fn digits_place(value: &str) -> u8 {
    match SYSTEMS.iter().position(|(name, _)| *name == value) {
        Some(i) => u8::try_from(i).unwrap_or(0),
        None if ten(value).is_some() => LITERAL,
        None => 0,
    }
}

/// Where escaped text of `spans` ends in a catalog's escaped text.
fn escaped_end(spans: &[Span]) -> u32 {
    spans
        .iter()
        .filter(|s| s.start & ESCAPED != 0)
        .map(|s| (s.start & !ESCAPED) + s.len)
        .max()
        .unwrap_or(0)
}

/// What a `[meta]` key of the catalog's description says: its place in
/// direction, script, digits, calendar and its value, 1-based. Values
/// this release does not know are left as not said (0): a newer catalog
/// still loads.
fn described(key: &str, value: &str) -> Option<(usize, u8)> {
    Some(match key {
        "direction" => (
            0,
            match value {
                "ltr" => 1,
                "rtl" => 2,
                _ => 0,
            },
        ),
        "script" => (
            1,
            paperpaw_lang::scripts()
                .position(|s| s == value)
                .and_then(|i| u8::try_from(i + 1).ok())
                .unwrap_or(0),
        ),
        "digits" => (2, digits_place(value)),
        "calendar" => (3, place(&CALENDARS, value)),
        _ => return None,
    })
}

/// The place of `value` in `names`, 0 if it is not there.
fn place(names: &[&str], value: &str) -> u8 {
    names
        .iter()
        .position(|n| *n == value)
        .and_then(|i| u8::try_from(i).ok())
        .unwrap_or(0)
}

/// A catalog of a language not in use, kept small: its source and where
/// its language and name are in it, nothing more (see
/// [`Translator`](crate::Translator)). Made a catalog again when its
/// language comes into use.
#[derive(Debug, Clone)]
pub(crate) struct Parked {
    source: Cow<'static, str>,
    language: Span,
    name: Span,
    pub(crate) added: u32,
}

impl Parked {
    pub(crate) fn language(&self) -> &str {
        self.source.get(self.language.range()).unwrap_or_default()
    }

    pub(crate) fn name(&self) -> &str {
        self.source.get(self.name.range()).unwrap_or_default()
    }

    /// The catalog again, unloaded (its index made when it loads).
    pub(crate) fn unpark(self) -> Catalog {
        let added = self.added;
        // Read once already: it parses.
        let mut catalog = Catalog::parse(self.source).unwrap_or_else(|_| Catalog::empty());
        catalog.unload();
        catalog.added = added;
        catalog
    }
}

/// A byte range in the source, or in `escaped` if the top bit is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: u32,
    len: u32,
}

impl Span {
    /// Its bytes in the source (none for one in `escaped`).
    fn range(self) -> core::ops::Range<usize> {
        match self.start & ESCAPED {
            0 => self.start as usize..(self.start + self.len) as usize,
            _ => 0..0,
        }
    }
}

const ESCAPED: u32 = 1 << 31;
/// In the index: a message of `others`.
const OTHER: u32 = 1 << 31;

/// The index of a catalog: its entries (see [`OTHER`]) in two bytes each
/// where they fit, as in most catalogs (of a source under 32 KiB); else
/// (a wide index, see `Catalog::wide`) each in four, the low half first.
/// Made at run time, or kept in the program for catalogs built in
/// ([`FixedIndexes`]).
#[derive(Debug, Clone, Default)]
struct Index(Cow<'static, [u16]>);

/// The indexes of catalogs built into the program, made when it was built
/// (`paperpaw-app`'s build script): each with the [`fingerprint`] of its
/// source. A catalog whose source has one uses it rather than an index
/// on the heap ([`Translator::fixed_indexes`](crate::Translator::fixed_indexes)).
pub type FixedIndexes = &'static [(u64, &'static [u16])];

/// What a catalog's source is known by in [`FixedIndexes`]: FNV-1a of its
/// bytes.
pub fn fingerprint(source: &str) -> u64 {
    source.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// In a narrow index: a message of `others`.
const NARROW_OTHER: u16 = 1 << 15;

impl Index {
    /// `entries` (sorted), narrow where every one fits; and whether it is
    /// wide.
    fn of(entries: Vec<u32>) -> (Self, bool) {
        let narrow = |at: u32| -> Option<u16> {
            let place = at & !OTHER;
            let place = u16::try_from(place)
                .ok()
                .filter(|p| p & NARROW_OTHER == 0)?;
            Some(if at & OTHER != 0 {
                place | NARROW_OTHER
            } else {
                place
            })
        };
        match entries
            .iter()
            .map(|at| narrow(*at))
            .collect::<Option<Vec<u16>>>()
        {
            Some(narrow) => (
                Index(Cow::Owned(narrow.into_boxed_slice().into_vec())),
                false,
            ),
            None => {
                let halves = entries
                    .iter()
                    .flat_map(|at| [*at as u16, (at >> 16) as u16])
                    .collect::<Vec<u16>>();
                (
                    Index(Cow::Owned(halves.into_boxed_slice().into_vec())),
                    true,
                )
            }
        }
    }

    fn len(&self, wide: bool) -> usize {
        if wide { self.0.len() / 2 } else { self.0.len() }
    }

    /// Entry `i`, as a wide index has it.
    fn at(&self, i: usize, wide: bool) -> u32 {
        if wide {
            return u32::from(self.0[2 * i]) | u32::from(self.0[2 * i + 1]) << 16;
        }
        let at = self.0[i];
        let place = u32::from(at & !NARROW_OTHER);
        if at & NARROW_OTHER != 0 {
            place | OTHER
        } else {
            place
        }
    }
}

/// A character of a key on a plain message line: a letter, a digit, `_`,
/// `-` or `.`. A message line is plain when the parser reads it as these
/// do: the key's characters, `=`, then the rest of the line, trimmed and
/// not quoted. The index keeps any other message in `others`.
fn is_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

/// The key of the plain message line starting at `start`. Only the key
/// is read, as looking a message up compares many keys and reads one
/// value: the catalog may lie in flash, where every byte read costs.
fn plain_key(source: &str, start: u32) -> &str {
    let line = source.get(start as usize..).unwrap_or_default();
    let len = line
        .bytes()
        .position(|b| !is_key_byte(b))
        .unwrap_or(line.len());
    &line[..len]
}

/// How the key of the plain message line starting at `start` compares
/// with `key`, as `plain_key(source, start).cmp(key)` does. It reads the
/// line only up to where the two differ.
fn cmp_plain_key(source: &str, start: u32, key: &str) -> Ordering {
    let line = source.get(start as usize..).unwrap_or_default();
    let mut theirs = line.bytes().take_while(|b| is_key_byte(*b));
    let mut ours = key.bytes();
    loop {
        match (theirs.next(), ours.next()) {
            (Some(a), Some(b)) if a == b => {}
            (a, b) => return a.cmp(&b),
        }
    }
}

/// The value of the plain message line starting at `start`.
fn plain_value(source: &str, start: u32) -> Option<&str> {
    let after = start as usize + plain_key(source, start).len();
    let rest = source.get(after..)?;
    let rest = &rest[..rest.find('\n').unwrap_or(rest.len())];
    let value = rest.trim_start().strip_prefix('=')?.trim();
    (!value.starts_with('"')).then_some(value)
}

/// Why a catalog could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CatalogError {
    Syntax(ParseError),
    /// `[meta] language` is missing.
    MissingLanguage,
    /// `[meta] plural` names an unknown rule.
    UnknownPluralRule {
        line: u32,
    },
    /// A key outside the `[meta]` and `[messages]` sections.
    UnexpectedSection {
        line: u32,
    },
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Syntax(error) => error.fmt(f),
            CatalogError::MissingLanguage => f.write_str("missing `language` in [meta]"),
            CatalogError::UnknownPluralRule { line } => {
                write!(f, "line {line}: unknown plural rule")
            }
            CatalogError::UnexpectedSection { line } => {
                write!(f, "line {line}: only [meta] and [messages] are allowed")
            }
            CatalogError::Other(code) => write!(f, "catalog error {code}"),
        }
    }
}

impl core::error::Error for CatalogError {}

impl Catalog {
    /// Parses a catalog file. Built-in catalogs (`&'static str`) are not
    /// copied.
    pub fn parse(source: impl Into<Cow<'static, str>>) -> Result<Catalog, CatalogError> {
        let mut catalog = Catalog {
            source: source.into(),
            language: Span { start: 0, len: 0 },
            name: Span { start: 0, len: 0 },
            plural: PluralRule::OneOther,
            direction: 0,
            script: 0,
            digits: 0,
            calendar: 0,
            escaped: Box::default(),
            meta_len: 0,
            index: Index::default(),
            others: Box::default(),
            loaded: false,
            wide: false,
            added: 0,
        };
        catalog.scan(true)?;
        Ok(catalog)
    }

    /// A catalog of no messages, of the fallback language.
    fn empty() -> Catalog {
        Catalog::parse("[meta]\nlanguage = en\n").unwrap_or_else(|_| unreachable!())
    }

    /// Kept small while its language is not in use ([`Parked`]); itself
    /// where its language or name is escaped (they would be lost).
    pub(crate) fn park(self) -> Result<Parked, Catalog> {
        if self.meta_len > 0 {
            return Err(self);
        }
        Ok(Parked {
            source: self.source,
            language: self.language,
            name: self.name,
            added: self.added,
        })
    }

    /// Reads the source. With `meta`, also reads and validates `[meta]`
    /// (otherwise only the messages are indexed again).
    fn scan(&mut self, meta: bool) -> Result<(), CatalogError> {
        let source: &str = &self.source;
        let base = source.as_ptr() as usize;
        let mut escaped = if meta {
            String::new()
        } else {
            let mut kept = String::from(core::mem::take(&mut self.escaped));
            kept.truncate(self.meta_len as usize);
            kept
        };
        let span = |escaped: &mut String, value: Cow<'_, str>| match value {
            Cow::Borrowed(text) => Span {
                start: (text.as_ptr() as usize - base) as u32,
                len: text.len() as u32,
            },
            Cow::Owned(text) => {
                let start = escaped.len() as u32 | ESCAPED;
                escaped.push_str(&text);
                Span {
                    start,
                    len: text.len() as u32,
                }
            }
        };
        let mut index = Vec::new();
        let mut others = Vec::new();
        let (mut language, mut name, mut plural) = (None, None, PluralRule::OneOther);
        // Direction, script, digits, calendar.
        let mut said = [0u8; 4];
        for entry in paperpaw_config::parse(source) {
            let entry = entry.map_err(CatalogError::Syntax)?;
            match (entry.section, entry.key) {
                ("meta", _) if !meta => {}
                ("meta", "language") => language = Some(span(&mut escaped, entry.value.as_str())),
                ("meta", "name") => name = Some(span(&mut escaped, entry.value.as_str())),
                ("meta", "plural") => {
                    plural = PluralRule::from_id(&entry.value.as_str())
                        .ok_or(CatalogError::UnknownPluralRule { line: entry.line })?;
                }
                ("meta", key) => {
                    if let Some((at, value)) = described(key, &entry.value.as_str()) {
                        said[at] = value;
                    }
                }
                ("messages", key) => {
                    let value = entry.value.as_str();
                    // Plainly on its line: only where the line starts. The
                    // parser gives a key of key characters or `@`, a quoted
                    // value right after its `"` and a plain one after `=`
                    // or a space.
                    let start = (key.as_ptr() as usize - base) as u32;
                    let unquoted = |v: &str| {
                        (v.as_ptr() as usize - base)
                            .checked_sub(1)
                            .and_then(|before| source.as_bytes().get(before))
                            .is_some_and(|b| *b != b'"')
                    };
                    match &value {
                        Cow::Borrowed(v) if key.bytes().all(is_key_byte) && unquoted(v) => {
                            index.push(start);
                        }
                        _ => {
                            index.push(others.len() as u32 | OTHER);
                            let key = span(&mut escaped, Cow::Borrowed(key));
                            others.push((key, span(&mut escaped, value)));
                        }
                    }
                }
                _ => return Err(CatalogError::UnexpectedSection { line: entry.line }),
            }
        }
        if meta {
            // Meta values are read before any message, so an escaped one
            // is at the start of `escaped`.
            self.language = language.ok_or(CatalogError::MissingLanguage)?;
            self.name = name.unwrap_or(self.language);
            self.plural = plural;
            [self.direction, self.script, self.digits, self.calendar] = said;
            self.meta_len = escaped_end(&[self.language, self.name]);
        }
        // Sorted by key, each read once, then by where its line is, so
        // that of equal keys the last in the file wins. Unstable: a stable
        // sort takes 4 KiB of stack (too much on the X4). Every key's line
        // is in the source, a quoted one's as its key's span says.
        let line = |at: u32| match at & OTHER {
            0 => at,
            _ => others
                .get((at & !OTHER) as usize)
                .map_or(0, |(key, _)| key.start),
        };
        let mut keyed: Vec<(&str, u32)> = index
            .iter()
            .map(|&at| (key_of(source, &escaped, &others, at), at))
            .collect();
        keyed.sort_unstable_by(|a, b| a.0.cmp(b.0).then_with(|| line(a.1).cmp(&line(b.1))));
        index.clear();
        let mut sorted = keyed.iter().peekable();
        while let Some(&(key, at)) = sorted.next() {
            if sorted.peek().is_none_or(|next| next.0 != key) {
                index.push(at);
            }
        }
        drop(keyed);
        self.escaped = escaped.into_boxed_str();
        // Boxed: they no longer change, and a box is smaller than a
        // vector (one of each per catalog, of every plugin).
        (self.index, self.wide) = Index::of(index);
        self.others = others.into_boxed_slice();
        self.loaded = true;
        Ok(())
    }

    /// Drops the message index (keeps language, name and plural rule).
    pub fn unload(&mut self) {
        self.index = Index::default();
        self.wide = false;
        self.others = Box::default();
        self.escaped = Box::from(&self.escaped[..self.meta_len as usize]);
        self.loaded = false;
    }

    /// Rebuilds the message index after [`Catalog::unload`].
    pub fn load(&mut self) {
        if !self.loaded {
            // The source was validated when it was parsed.
            let _ = self.scan(false);
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn text(&self, span: Span) -> &str {
        text_of(&self.source, &self.escaped, span)
    }

    /// Language tag, e.g. `de` or `pt-BR`.
    pub fn language(&self) -> &str {
        self.text(self.language)
    }

    /// Human-readable language name in that language, e.g. `Deutsch`.
    pub fn name(&self) -> &str {
        self.text(self.name)
    }

    pub fn plural_rule(&self) -> PluralRule {
        self.plural
    }

    /// The direction of the interface in its language (`[meta]
    /// direction`): [`Direction::Auto`](paperpaw_lang::Direction) where
    /// the catalog does not say.
    pub fn direction(&self) -> paperpaw_lang::Direction {
        match self.direction {
            1 => paperpaw_lang::Direction::Ltr,
            2 => paperpaw_lang::Direction::Rtl,
            _ => paperpaw_lang::Direction::Auto,
        }
    }

    /// The script its language is written in (`[meta] script`), as
    /// ISO 15924 names it, e.g. `Arab`, if it says.
    pub fn script(&self) -> Option<&'static str> {
        let i = usize::from(self.script).checked_sub(1)?;
        paperpaw_lang::scripts().nth(i)
    }

    /// The digits numbers are written with, 0 to 9 (`[meta] digits`: a
    /// numbering system's CLDR name, see [`digit_systems`], or the ten
    /// digits themselves). Latin digits where the catalog says none, names
    /// a system this release does not know, or gives other than ten
    /// characters. Ten digits given are read from the source again on each
    /// call: keep the result.
    pub fn digits(&self) -> [char; 10] {
        match self.digits {
            LITERAL => self.literal_digits(),
            place => system_digits(place),
        }
        .unwrap_or(LATIN)
    }

    /// The ten digits `[meta] digits` gives (the last such line).
    fn literal_digits(&self) -> Option<[char; 10]> {
        paperpaw_config::parse(&self.source)
            .map_while(Result::ok)
            .filter(|entry| entry.section == "meta" && entry.key == "digits")
            .last()
            .and_then(|entry| ten(&entry.value.as_str()))
    }

    /// The calendar dates are given in (`[meta] calendar`), as the CLDR
    /// names it (see [`calendars`]): `gregorian` where it does not say.
    pub fn calendar(&self) -> &'static str {
        CALENDARS
            .get(usize::from(self.calendar))
            .copied()
            .unwrap_or(CALENDARS[0])
    }

    fn key(&self, at: u32) -> &str {
        key_of(&self.source, &self.escaped, &self.others, at)
    }

    /// How the key of index entry `at` compares with `key`.
    fn cmp_key(&self, at: u32, key: &str) -> Ordering {
        if at & OTHER != 0 {
            self.key(at).cmp(key)
        } else {
            cmp_plain_key(&self.source, at, key)
        }
    }

    /// The message for `key`, if this catalog has one and is loaded.
    pub fn get(&self, key: &str) -> Option<&str> {
        let len = self.index.len(self.wide);
        // A key before the first or after the last is not here. A catalog
        // mostly holds one plugin's keys, so a translator looking through
        // all of them stops at this in most.
        let last = self.index.at(len.checked_sub(1)?, self.wide);
        let first = self.index.at(0, self.wide);
        if self.cmp_key(first, key).is_gt() || self.cmp_key(last, key).is_lt() {
            return None;
        }
        let (mut low, mut high) = (0, len);
        let at = loop {
            if low >= high {
                return None;
            }
            let middle = low + (high - low) / 2;
            let at = self.index.at(middle, self.wide);
            match self.cmp_key(at, key) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => break at,
            }
        };
        if at & OTHER != 0 {
            let (_, value) = self.others.get((at & !OTHER) as usize)?;
            Some(self.text(*value))
        } else {
            plain_value(&self.source, at)
        }
    }

    /// All message keys, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        (0..self.index.len(self.wide)).map(|i| self.key(self.index.at(i, self.wide)))
    }

    pub fn len(&self) -> usize {
        self.index.len(self.wide)
    }

    pub fn is_empty(&self) -> bool {
        self.index.0.is_empty()
    }

    /// Its index, two bytes an entry, for a build script that keeps it in
    /// the program ([`FixedIndexes`]); `None` while unloaded or where the
    /// index is wide.
    pub fn narrow_index(&self) -> Option<&[u16]> {
        (self.loaded && !self.wide).then_some(&*self.index.0)
    }

    /// The bytes its index takes on the heap: none where it is kept in
    /// the program ([`FixedIndexes`]) or unloaded.
    pub fn index_on_heap(&self) -> usize {
        match &self.index.0 {
            Cow::Owned(index) => index.len() * 2,
            Cow::Borrowed(_) => 0,
        }
    }

    /// Takes its index from `fixed` where that has the same one for its
    /// source: the one on the heap goes.
    pub fn use_fixed(&mut self, fixed: FixedIndexes) {
        if fixed.is_empty() || !matches!(self.index.0, Cow::Owned(_)) {
            return;
        }
        let Some(own) = self.narrow_index() else {
            return;
        };
        let print = fingerprint(&self.source);
        if let Some(&(_, index)) = fixed.iter().find(|(p, index)| *p == print && *index == own) {
            self.index = Index(Cow::Borrowed(index));
        }
    }
}

/// The key of index entry `at` (see the field `index` of `Catalog`).
fn key_of<'a>(source: &'a str, escaped: &'a str, others: &[(Span, Span)], at: u32) -> &'a str {
    if at & OTHER != 0 {
        others
            .get((at & !OTHER) as usize)
            .map_or("", |(key, _)| text_of(source, escaped, *key))
    } else {
        plain_key(source, at)
    }
}

fn text_of<'a>(source: &'a str, escaped: &'a str, span: Span) -> &'a str {
    let (text, start) = if span.start & ESCAPED != 0 {
        (escaped, span.start & !ESCAPED)
    } else {
        (source, span.start)
    };
    text.get(start as usize..(start + span.len) as usize)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A catalog of a source past 32 KiB has a wide index: every message
    /// is found, plain or quoted, before and after unloading.
    #[test]
    fn a_large_catalog_finds_every_message() {
        let mut source = String::from(
            "[meta]
language = en
[messages]
",
        );
        for n in 0..3000 {
            source.push_str(&alloc::format!(
                "message.{n:04} = The text of message {n}.
"
            ));
        }
        source.push_str(
            "quoted.last = \"  far \"
",
        );
        assert!(source.len() > 64 * 1024);
        let mut catalog = Catalog::parse(source).unwrap();
        assert!(catalog.wide);
        for _ in 0..2 {
            assert_eq!(catalog.len(), 3001);
            assert_eq!(catalog.get("message.0000"), Some("The text of message 0."));
            assert_eq!(
                catalog.get("message.2999"),
                Some("The text of message 2999.")
            );
            assert_eq!(catalog.get("quoted.last"), Some("  far "));
            assert_eq!(catalog.get("message.3000"), None);
            assert_eq!(catalog.keys().next(), Some("message.0000"));
            catalog.unload();
            assert!(catalog.is_empty());
            catalog.load();
        }
    }

    #[test]
    fn parses_meta_and_messages() {
        let catalog = Catalog::parse(
            "[meta]\nlanguage = de\nname = Deutsch\n[messages]\nb = Zwei\na = Eins\n",
        )
        .unwrap();
        assert_eq!(catalog.language(), "de");
        assert_eq!(catalog.name(), "Deutsch");
        assert_eq!(catalog.get("a"), Some("Eins"));
        assert_eq!(catalog.get("b"), Some("Zwei"));
        assert_eq!(catalog.get("c"), None);
        assert_eq!(catalog.keys().collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn escaped_values_survive_unloading() {
        // Owned sources (e.g. loaded at runtime) work the same way.
        let mut catalog = Catalog::parse(String::from(
            r#"[meta]
language = "d\\e"
[messages]
b = plain
a = "two\nlines"
"#,
        ))
        .unwrap();
        assert_eq!(catalog.language(), r"d\e");
        assert_eq!(catalog.get("a"), Some("two\nlines"));
        catalog.unload();
        assert_eq!(catalog.get("a"), None);
        assert_eq!(catalog.language(), r"d\e");
        catalog.load();
        assert_eq!(catalog.get("a"), Some("two\nlines"));
        assert_eq!(catalog.get("b"), Some("plain"));
    }

    #[test]
    fn plain_and_quoted_messages() {
        let catalog = Catalog::parse(
            "[meta]\nlanguage = en\n[messages]\n  spaced   =   value with = and # in it  \r\n\
             quoted = \"  kept spaces  \"\nescaped = \"a\\tb\"\nempty =\nlast = end",
        )
        .unwrap();
        assert_eq!(catalog.get("spaced"), Some("value with = and # in it"));
        assert_eq!(catalog.get("quoted"), Some("  kept spaces  "));
        assert_eq!(catalog.get("escaped"), Some("a\tb"));
        assert_eq!(catalog.get("empty"), Some(""));
        assert_eq!(catalog.get("last"), Some("end"));
        // Only the quoted ones need more than where their line starts.
        assert_eq!(catalog.others.len(), 2);
        assert_eq!(
            catalog.keys().collect::<Vec<_>>(),
            ["empty", "escaped", "last", "quoted", "spaced"]
        );
    }

    #[test]
    fn last_duplicate_wins() {
        let catalog =
            Catalog::parse("[meta]\nlanguage = en\n[messages]\na = first\na = second\n").unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.get("a"), Some("second"));
        // Also where one of them is quoted, before or after.
        let catalog = Catalog::parse(
            "[meta]\nlanguage = en\n[messages]\nb = \"one\"\na = first\nb = two\na = \"second\"\n",
        )
        .unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.get("a"), Some("second"));
        assert_eq!(catalog.get("b"), Some("two"));
    }

    #[test]
    fn rejects_invalid_catalogs() {
        assert_eq!(
            Catalog::parse("[messages]\na = b\n").unwrap_err(),
            CatalogError::MissingLanguage
        );
        assert_eq!(
            Catalog::parse("[meta]\nlanguage = en\nplural = klingon\n").unwrap_err(),
            CatalogError::UnknownPluralRule { line: 3 }
        );
        assert_eq!(
            Catalog::parse("[meta]\nlanguage = en\n[other]\na = b\n").unwrap_err(),
            CatalogError::UnexpectedSection { line: 4 }
        );
    }
}
