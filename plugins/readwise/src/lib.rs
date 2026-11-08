//! Readwise: the highlights of a book go to Readwise when its notes are
//! exported (*Export as Markdown* at the end of a book's bookmarks and
//! highlights), with the notes on them and where they are in the book.
//! Readwise keeps each highlight once, so exporting again sends only
//! what is new.
//!
//! Readwise needs one's access token (readwise.io/access_token): the
//! first export asks for it on the keyboard (or a phone, where offered);
//! it is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`). Shipped switched off: switch it on in the
//! plugins.
//!
//! What cannot be sent (no connection, Readwise down) waits and is sent
//! later: with the next export, and when Wi-Fi comes back where PaperPaw
//! manages it. The notes are read a line at a time and the request
//! written to a file and sent from there, so notes of any length are
//! sent on small devices too.
//!
//! It reads the Markdown the reader writes (see [`Parser`]), so it works
//! with any reader that writes the same and emits
//! `core.notes-exported`.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[cfg(test)]
mod native_parity_tests;
pub mod native_readwise;
mod send;

use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::RefCell;

use paperpaw_sdk::builtin::events;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::json;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::{Command, Commands, Id, Manifest, Plugin, RegisterError, Registrar};

use send::{Run, Start};

pub const PLUGIN: Id = Id::new_static("readwise");
/// The access token, among the passwords and keys.
pub const SECRET: &str = "readwise.token";
/// The token, entered on the keyboard.
const TOKEN_ENTERED: Event<Typed> = Event::new("readwise.token-entered");
/// Longest text and note of a highlight, title and author sent, in bytes
/// (Readwise takes no more characters).
const MAX_TEXT: usize = 8191;
const MAX_TITLE: usize = 511;
const MAX_AUTHOR: usize = 1024;

/// A highlight as the Markdown has it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Highlight {
    pub text: String,
    /// Where in the book, in percent.
    pub percent: u32,
    pub note: String,
}

/// Where a [`Parser`] is in the notes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The title and author, before the first heading of a part.
    #[default]
    Head,
    /// The highlights, under the first heading.
    Highlights,
    /// Later parts (bookmarks), left out.
    Rest,
}

/// Reads the Markdown the reader writes, a line at a time: `# Title`, the
/// author on the next line, then under the first `##` heading each
/// highlight as a quote (`> text`) ending in its place (`> 12 %`),
/// followed by the note on it, if any; later headings (bookmarks) are
/// left out. Only the highlight being read is held, cut to what Readwise
/// takes.
#[derive(Debug, Default)]
pub struct Parser {
    part: Part,
    title: String,
    author: String,
    current: Option<Highlight>,
    quoting: bool,
}

impl Parser {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    /// Reads the next line (without its line break): the highlight it
    /// ends, if any.
    pub fn line(&mut self, line: &str) -> Option<Highlight> {
        match self.part {
            Part::Head => {
                if line.starts_with("## ") {
                    self.part = Part::Highlights;
                } else if let Some(title) = line.strip_prefix("# ") {
                    self.title.clear();
                    push_bounded(&mut self.title, title.trim(), MAX_TITLE);
                } else if !line.trim().is_empty()
                    && self.author.is_empty()
                    && !self.title.is_empty()
                {
                    push_bounded(&mut self.author, line.trim(), MAX_AUTHOR);
                }
                None
            }
            Part::Highlights if line.starts_with("## ") => {
                let last = self.finish();
                self.part = Part::Rest;
                last
            }
            Part::Highlights => self.highlight_line(line),
            Part::Rest => None,
        }
    }

    fn highlight_line(&mut self, line: &str) -> Option<Highlight> {
        if let Some(quote) = line.strip_prefix('>') {
            let quote = quote.trim();
            // A quote after other lines starts the next highlight.
            let ended = if self.quoting {
                None
            } else {
                self.quoting = true;
                self.current.replace(Highlight::default())
            };
            let h = self.current.get_or_insert_with(Highlight::default);
            if let Some(percent) = quote.strip_suffix('%').and_then(|p| p.trim().parse().ok()) {
                h.percent = percent;
            } else if !quote.is_empty() {
                push_line(&mut h.text, quote);
            }
            return ended.filter(|h| !h.text.is_empty());
        }
        self.quoting = false;
        if let Some(h) = self.current.as_mut()
            && !line.trim().is_empty()
        {
            push_line(&mut h.note, line.trim());
        }
        None
    }

    /// The end of the notes: the highlight read last, if any.
    pub fn finish(&mut self) -> Option<Highlight> {
        self.quoting = false;
        self.current.take().filter(|h| !h.text.is_empty())
    }

    /// The highlight `h` of these notes as Readwise takes it, in JSON.
    pub fn json(&self, h: &Highlight) -> String {
        let mut json = format!(
            "{{\"text\":{},\"title\":{},\"source_type\":\"paperpaw\",\"category\":\"books\",\
             \"location\":{},\"location_type\":\"order\"",
            json::string(&h.text),
            json::string(&self.title),
            h.percent
        );
        if !self.author.is_empty() {
            json.push_str(&format!(",\"author\":{}", json::string(&self.author)));
        }
        if !h.note.is_empty() {
            json.push_str(&format!(",\"note\":{}", json::string(&h.note)));
        }
        json.push('}');
        json
    }
}

/// Adds `text` to `field` on a line of its own, as much as fits in
/// [`MAX_TEXT`] bytes.
fn push_line(field: &mut String, text: &str) {
    let before = field.len();
    if before > 0 {
        field.push('\n');
    }
    push_bounded(field, text, MAX_TEXT);
    // Nothing of it fit.
    if before > 0 && field.len() == before + 1 {
        field.truncate(before);
    }
}

/// Adds `text` to `field`, as much as fits in `max` bytes, whole
/// characters only.
fn push_bounded(field: &mut String, text: &str, max: usize) {
    let mut end = text.len().min(max.saturating_sub(field.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    field.push_str(&text[..end]);
}

/// The Readwise plugin.
#[derive(Debug, Default)]
pub struct Readwise;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Readwise {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Highlights export (cloud)",
            paperpaw_sdk::package_version!(),
        )
        .description("Sends highlights to Readwise.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let run: Rc<RefCell<Run>> = Rc::default();
        // Any reader's notes, once exported as Markdown.
        let state = run.clone();
        r.on(events::NOTES_EXPORTED, move |env, exported, out| {
            if send::add_waiting(env.storage(), &exported.path).is_ok() {
                state.borrow_mut().start(env, Start::Export, None, out);
                retry_later(env, out);
            }
        })?;
        // Exports still waiting: at the next connection, at the latest in
        // half an hour (longer while saving).
        let state = run.clone();
        r.on(RETRY_DUE, move |env, (), out| {
            state.borrow_mut().start(env, Start::Retry, None, out);
            retry_later(env, out);
        })?;
        r.on(events::STARTED, |env, _, out| retry_later(env, out))?;
        let state = run.clone();
        r.on(TOKEN_ENTERED, move |env, typed, out| {
            let token = typed.text.trim();
            if token.is_empty() {
                return;
            }
            out.push(Command::set_secret(SecretChange::keep(
                SECRET,
                "readwise.token",
                token,
            )));
            // Not kept yet while this runs: sent with it directly.
            state
                .borrow_mut()
                .start(env, Start::Export, Some(token), out);
        })?;
        let state = run.clone();
        r.on(send::SENT, move |env, reply, out| {
            state.borrow_mut().answered(env, reply, out);
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Declared work: exports still waiting, sent again.
const RETRY_DUE: paperpaw_sdk::event::Event = paperpaw_sdk::event::Event::new("readwise.retry");

/// Declares the next try while exports wait.
fn retry_later(env: &paperpaw_sdk::Env<'_>, out: &mut Commands) {
    if !send::waiting(env.storage()).is_empty() {
        let work = paperpaw_sdk::work::Work::network(RETRY_DUE.id().clone())
            .within(core::time::Duration::from_secs(30 * 60))
            .at_moments(paperpaw_sdk::work::Moments::ONLINE);
        out.push(Command::work(work));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use paperpaw_i18n::{Catalog, check};

    const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
        > Curiouser and curiouser!\n>\n> 12 %\n\n\
        > Who are you?\n>\n> 40 %\n\nThe caterpillar asks.\nTwice.\n\n\
        ## Bookmarks\n\n- 50 %: Down the rabbit hole\n";

    /// The parser after `text`, and the highlights it gave.
    fn parse(text: &str) -> (Parser, Vec<Highlight>) {
        let mut parser = Parser::default();
        let mut highlights: Vec<Highlight> = text.lines().filter_map(|l| parser.line(l)).collect();
        highlights.extend(parser.finish());
        (parser, highlights)
    }

    #[test]
    fn reads_the_readers_markdown() {
        let (notes, highlights) = parse(NOTES);
        assert_eq!(notes.title(), "Alice's Adventures");
        assert_eq!(notes.author(), "Lewis Carroll");
        assert_eq!(
            highlights,
            [
                Highlight {
                    text: String::from("Curiouser and curiouser!"),
                    percent: 12,
                    note: String::new(),
                },
                Highlight {
                    text: String::from("Who are you?"),
                    percent: 40,
                    note: String::from("The caterpillar asks.\nTwice."),
                },
            ]
        );
        // The last highlight, with no heading after it.
        let (_, highlights) = parse(&NOTES[..NOTES.find("## Bookmarks").unwrap()]);
        assert_eq!(highlights.len(), 2);
        // Without an author or highlights.
        let (notes, highlights) = parse("# Title\n\n## Bookmarks\n\n- 5 %: x\n");
        assert_eq!((notes.title(), notes.author()), ("Title", ""));
        assert!(highlights.is_empty());
    }

    #[test]
    fn writes_json_for_readwise() {
        let (notes, highlights) = parse(NOTES);
        let json = notes.json(&highlights[1]);
        assert!(json.starts_with("{\"text\":\"Who are you?\",\"title\":\"Alice's Adventures\""));
        assert!(json.contains("\"author\":\"Lewis Carroll\""));
        assert!(json.contains("\"note\":\"The caterpillar asks.\\nTwice.\""));
        assert!(json.contains("\"location\":40,\"location_type\":\"order\""));
        assert!(!notes.json(&highlights[0]).contains("\"note\""));
    }

    #[test]
    fn long_highlights_are_cut_to_what_readwise_takes() {
        let long = "\u{e4}".repeat(MAX_TEXT);
        let text = format!("# T\n\n## Highlights\n\n> {long}\n> more\n>\n> 5 %\n\n{long}\n");
        let (_, highlights) = parse(&text);
        let h = &highlights[0];
        assert_eq!(h.text.len(), MAX_TEXT - 1);
        assert!(h.text.chars().all(|c| c == '\u{e4}'));
        assert_eq!(h.note.len(), MAX_TEXT - 1);
        assert_eq!(h.percent, 5);
        let (notes, _) = parse(&format!("# {}\n\n{}\n", "t".repeat(4096), "a".repeat(4096)));
        assert_eq!(notes.title().len(), MAX_TITLE);
        assert_eq!(notes.author().len(), MAX_AUTHOR);
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
