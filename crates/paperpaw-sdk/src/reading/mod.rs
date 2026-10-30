//! Building blocks of reading, for readers and for the plugins around
//! them: a book through the plugins' text filters ([`FilteredDocument`]),
//! the starts of its pages kept on the device ([`PageIndex`]), how it is
//! opened ([`open_options`]); with the feature `unstable` (as the layout
//! it builds on, until 1.0) the type of its pages and the page drawn
//! (`PageType`, `ReadingFaces`, `word_boxes`, `draw_lines`).
//!
//! The words of the page shown: any plugin asks the reader with
//! [`ask_page_words`] and hears them as [`PageWords`] (a dictionary that
//! lists the page's hard words, a summary, a word count). The reader
//! answers while a book is open; nothing comes otherwise. Decorators get
//! the words with every page without asking (see
//! [`decorations`](crate::decorations)).

mod filtered;
mod index;
#[cfg(feature = "unstable")]
mod typeset;

pub use filtered::{DroppedRuns, FilteredDocument};
pub use index::PageIndex;
#[cfg(feature = "unstable")]
pub use typeset::{
    EmbeddedFonts, PageType, ReadingFaces, StatusRoom, draw_image, draw_lines, small_caps_parts,
    uses_book_fonts, word_boxes,
};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::events;
use crate::document::Locator;
use crate::event::{Event, EventData};
use crate::{Command, Params, Rect};

/// A word on the page, with its box on the screen and its place in the
/// book (`word_boxes`, page layers' [`PageView::words`](crate::layers::PageView::words)).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WordBox {
    pub area: Rect,
    pub text: String,
    pub start: Locator,
    pub end: Locator,
    /// The word runs right to left (Arabic, Hebrew): its start is at the
    /// right of its box, so a selection from it grows to the left.
    pub rtl: bool,
}

impl WordBox {
    pub fn new(area: Rect, text: impl Into<String>, start: Locator, end: Locator) -> Self {
        Self {
            area,
            text: text.into(),
            start,
            end,
            rtl: false,
        }
    }

    /// The same, running right to left or not (see [`WordBox::rtl`]).
    #[must_use]
    pub fn rtl(mut self, rtl: bool) -> Self {
        self.rtl = rtl;
        self
    }

    /// The word without the punctuation around it, as it is looked up.
    pub fn lookup_text(&self) -> &str {
        self.text.trim_matches(|c: char| !c.is_alphanumeric())
    }
}

/// A word of the page: its text and its place in the book. A word broken
/// over two lines is two words, each with the whole word as its text and
/// its own part's place (as [`PageWord`](crate::decorations::PageWord)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Word {
    pub text: String,
    pub start: Locator,
    /// Just after its last character.
    pub end: Locator,
}

impl Word {
    pub fn new(text: impl Into<String>, start: Locator, end: Locator) -> Self {
        Self {
            text: text.into(),
            start,
            end,
        }
    }
}

/// The words of the page a reader shows, in reading order, the answer to
/// [`ask_page_words`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageWords {
    /// The book.
    pub path: String,
    /// Its language, e.g. `de` (empty where unknown).
    pub language: String,
    /// Where the page starts and ends.
    pub start: Locator,
    pub end: Locator,
    pub words: Vec<Word>,
}

impl PageWords {
    pub fn new(
        path: impl Into<String>,
        language: impl Into<String>,
        start: Locator,
        end: Locator,
    ) -> Self {
        Self {
            path: path.into(),
            language: language.into(),
            start,
            end,
            words: Vec::new(),
        }
    }

    /// The same with `words`.
    pub fn words(mut self, words: Vec<Word>) -> Self {
        self.words = words;
        self
    }

    /// The page's text: its words with a space between.
    pub fn text(&self) -> String {
        let mut text = String::new();
        let mut last: Option<&Word> = None;
        for word in &self.words {
            // A broken word's second part: the whole word is there.
            if last.is_some_and(|l| l.text == word.text && l.end == word.start) {
                continue;
            }
            if last.is_some() {
                text.push(' ');
            }
            text.push_str(&word.text);
            last = Some(word);
        }
        text
    }
}

fn locator(text: &str) -> Option<Locator> {
    let (section, offset) = text.split_once(':')?;
    Some(Locator::new(section.parse().ok()?, offset.parse().ok()?))
}

impl EventData for PageWords {
    /// A word a line, `section:start:end:text` (words hold no line breaks).
    fn to_params(&self) -> Params {
        let mut words = String::new();
        for word in &self.words {
            let (start, end) = (word.start, word.end);
            words.push_str(&format!(
                "{}:{}:{}:{}:{}\n",
                start.section, start.offset, end.section, end.offset, word.text
            ));
        }
        let at = |l: Locator| format!("{}:{}", l.section, l.offset);
        Params::new()
            .with(crate::builtin::params::PATH, self.path.clone())
            .with(crate::builtin::params::LANGUAGE, self.language.clone())
            .with("start", at(self.start))
            .with("end", at(self.end))
            .with("words", words)
    }

    fn from_params(params: &Params) -> Option<Self> {
        let mut words = Vec::new();
        for line in params.get("words")?.lines() {
            let mut parts = line.splitn(5, ':');
            let mut number = || parts.next()?.parse::<u32>().ok();
            let start = Locator::new(number()?, number()?);
            let end = Locator::new(number()?, number()?);
            words.push(Word::new(parts.next()?.to_string(), start, end));
        }
        Some(
            PageWords::new(
                params.get(crate::builtin::params::PATH)?,
                params
                    .get(crate::builtin::params::LANGUAGE)
                    .unwrap_or_default(),
                locator(params.get("start")?)?,
                locator(params.get("end")?)?,
            )
            .words(words),
        )
    }
}

/// The text at the target of link `href` of `document` (the book as the
/// reader shows it, through the filters), as a footnote shows it: the
/// block there and the ones after it, up to a heading, a block left out
/// or a few hundred characters, on one line; `None` where the link leads
/// nowhere in the book, to a block left out or to no text.
pub fn note_text(
    document: &FilteredDocument<'_, '_>,
    storage: &dyn crate::Storage,
    href: &str,
) -> Option<String> {
    let mut at = document.document.link_target(storage, href)?;
    let mut text = String::new();
    for _ in 0..4 {
        let Ok(Some(block)) = document.kept_block_at(storage, at) else {
            break;
        };
        if matches!(block.kind, crate::document::BlockKind::Heading(_)) && !text.is_empty() {
            break;
        }
        let part = block.text.trim();
        if !part.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(part);
        }
        // A note that is only its number goes on in the next block.
        if text.chars().count() > 300 || text.chars().count() > 12 && block.end > block.start {
            break;
        }
        at = Locator::new(at.section, block.end);
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// How books are opened for reading: with their own styles as
/// [`settings::PUBLISHER_STYLES`](crate::builtin::settings::PUBLISHER_STYLES)
/// says, and the plugins' stylesheets. A plugin that opens the book a
/// reader shows (to search it, for its contents, to find an XPointer)
/// opens it so too, or its places need not fit the reader's, and a book
/// may be converted a second time.
pub fn open_options(env: &crate::Env<'_>) -> crate::OpenOptions {
    let styles = env
        .settings
        .read(&crate::builtin::settings::PUBLISHER_STYLES);
    env.formats()
        .options()
        .publisher_styles(styles)
        .css(env.stylesheets().join("\n"))
        .settings(format_settings(env))
}

/// The values of the format plugins' own settings (those in their
/// namespace), each book's own where it has them.
fn format_settings(env: &crate::Env<'_>) -> Vec<(String, String)> {
    let mut owners: Vec<&str> = env
        .formats
        .iter()
        .filter_map(|f| f.id.as_str().split_once('.').map(|(plugin, _)| plugin))
        .collect();
    owners.sort_unstable();
    owners.dedup();
    env.settings()
        .specs()
        .filter(|spec| {
            let id = spec.id.as_str();
            owners
                .iter()
                .any(|o| id.strip_prefix(o).is_some_and(|rest| rest.starts_with('.')))
        })
        .filter_map(|spec| {
            let id = spec.id.as_str();
            let value = env.settings().get(id)?.to_config_text();
            Some((id.to_string(), value))
        })
        .collect()
}

/// Which conversion of a book [`open_options`] make: places
/// ([`Locator`]s) are those of one conversion (an EPUB's with its own
/// styles holds style runs the one without them lacks, so its text is at
/// other places). Kept with a position (see
/// [`positions`](crate::positions)) to tell when it needs finding by its
/// XPointer.
pub fn conversion(env: &crate::Env<'_>) -> u64 {
    let styles = env
        .settings
        .read(&crate::builtin::settings::PUBLISHER_STYLES);
    env.stylesheets().iter().fold(
        paperpaw_digest::fnv1a64([u8::from(styles)]),
        |hash, sheet| hash.rotate_left(5) ^ paperpaw_digest::fnv1a64(sheet.as_bytes()),
    )
}

/// Asks the reader for the words of the page it shows: it answers with
/// `reply` (nothing comes where no book is open).
pub fn ask_page_words(reply: &Event<PageWords>) -> Command {
    events::PAGE_WORDS_WANTED.emit(events::PageWordsWanted::new(reply.id().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_words_go_through_an_event_whole() {
        let at = |o| Locator::new(3, o);
        let words = PageWords::new("books/a.epub", "en", at(0), at(30)).words(alloc::vec![
            Word::new("The", at(0), at(3)),
            Word::new("rabbit-hole:", at(4), at(16)),
            Word::new("Kaninchenbau", at(17), at(26)),
            Word::new("Kaninchenbau", at(26), at(29)),
        ]);
        let back = PageWords::from_params(&words.to_params()).unwrap();
        assert_eq!(back, words);
        assert_eq!(back.text(), "The rabbit-hole: Kaninchenbau");
        assert!(PageWords::from_params(&Params::new()).is_none());
    }

    #[test]
    fn asking_names_the_reply() {
        const MINE: Event<PageWords> = Event::new("my.words");
        let Command::Emit(event, params) = ask_page_words(&MINE) else {
            panic!("an event");
        };
        assert_eq!(event, *events::PAGE_WORDS_WANTED.id());
        let wanted = events::PAGE_WORDS_WANTED.read(&event, &params).unwrap();
        assert_eq!(wanted.reply, *MINE.id());
    }
}
