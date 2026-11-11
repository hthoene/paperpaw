//! The reader's side of the stage `reader.decorations@1` (`PORTING-7.md`): a
//! laid-out page is asked of the plugins that subscribed (a vocabulary list,
//! say), and the marks they answer are kept with the page, drawn with it and
//! hit by a tap. No host here: a fire is input bytes out and reply payloads
//! in (`native_view/decorations.rs` runs the stage), so all of it is tested.
//!
//! Memory (#248): the runs of a page (12 bytes each), at most [`MAX_MARKS`]
//! marks of 8 bytes and the looks they use are kept; the run texts are made
//! only for a fire and dropped before its reply buffer is taken.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{native::Error, reading::WordBox};

use crate::native_marks_codes as codes;
use crate::stages::reader_decorations_v1 as stage;

mod draw;
pub use draw::{Paint, draw_behind, draw_over};

/// The most marks a page keeps over all subscribers (about 2 KiB).
pub const MAX_MARKS: usize = 256;
/// The most runs one fire carries.
pub const RUNS_PER_FIRE: usize = 32;
/// The most looks a subscriber numbers (`look` 0 to 63).
const LOOKS: usize = 64;
/// The bytes of one packed mark: `run`, `from`, `to`, each u16 little-endian.
const MARK_BYTES: usize = 6;
/// Bytes a run row takes besides its text (tag, length, fields), at most.
const RUN_FRAME: usize = 24;
/// Bytes the head row takes besides its path and language, at most.
const PAGE_FRAME: usize = 24;

/// A run of the page: words `first..first + count` of one section, the
/// place of its first word, and the bytes of its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub section: u32,
    pub offset: u32,
    first: u16,
    count: u16,
    len: u16,
}

/// A mark: a byte range of a run, in one of the kept looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mark {
    run: u16,
    from: u16,
    to: u16,
    look: u8,
}

/// A look a kept mark uses, as its subscriber resolved it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    /// `<subscriber id>.<name>`: whose taps the marks are.
    pub role: String,
    pub background: u8,
    pub ink: u8,
    pub line: u8,
    pub line_tone: u8,
    pub thickness: u8,
    pub flags: u8,
}

/// What the page on show was answered: kept with the page, dropped with it.
#[derive(Debug, Default)]
pub struct Decorations {
    runs: Vec<Run>,
    looks: Vec<Look>,
    marks: Vec<Mark>,
    /// Marks left out past [`MAX_MARKS`] or failing their checks.
    pub dropped: u32,
}

/// One fire of a page: the input and the runs it carries.
#[derive(Debug)]
pub struct Fire {
    /// The encoded input (`decorate.page` and its `decorate.run` rows).
    pub input: Vec<u8>,
    /// The first of the page's runs this fire carries.
    pub first_run: usize,
    /// How many runs it carries.
    pub runs: usize,
}

/// A run's text appended to `to`: its words apart by one space; a word
/// broken over two lines (two boxes, one place) once, whole.
fn run_text(words: &[WordBox], run: &Run, to: &mut String) -> Result<(), Error> {
    let words = run_words(words, run);
    for n in 0..words.len() {
        if n > 0 && !continues(words, n) {
            push(to, " ")?;
        }
        push(to, piece(words, n))?;
    }
    Ok(())
}

/// Box `n` goes on the word of the box before it (a word broken over lines).
fn continues(words: &[WordBox], n: usize) -> bool {
    n > 0 && words.get(n - 1).map(|w| w.start) == words.get(n).map(|w| w.start)
}

/// The text box `n` gives its run: without the hyphen of a break.
fn piece(words: &[WordBox], n: usize) -> &str {
    let Some(word) = words.get(n) else {
        return "";
    };
    if continues(words, n + 1) {
        word.text.trim_end_matches(['-', '\u{AD}'])
    } else {
        word.text.as_str()
    }
}

fn run_words<'w>(words: &'w [WordBox], run: &Run) -> &'w [WordBox] {
    let first = usize::from(run.first);
    words
        .get(first..first + usize::from(run.count))
        .unwrap_or_default()
}

fn push(to: &mut String, text: &str) -> Result<(), Error> {
    to.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    to.push_str(text);
    Ok(())
}

/// Each word of `run` with its byte range in the run's text (both pieces of
/// a broken word get the whole word's range).
fn spans<'w>(
    words: &'w [WordBox],
    run: &Run,
) -> impl Iterator<Item = (&'w WordBox, u16, u16)> + 'w {
    let words = run_words(words, run);
    let mut at = 0_usize;
    let mut group = (0_usize, 0_usize);
    words.iter().enumerate().map(move |(n, word)| {
        if !continues(words, n) {
            if n > 0 {
                at += 1;
            }
            let start = at;
            at += piece(words, n).len();
            let mut next = n + 1;
            while continues(words, next) {
                at += piece(words, next).len();
                next += 1;
            }
            group = (start, at);
        }
        (word, clamp16(group.0), clamp16(group.1))
    })
}

fn clamp16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

impl Decorations {
    /// The runs of a page of `words`: one a section, split at a word where its
    /// text would pass the stage's 3,200 bytes.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(words: &[WordBox]) -> Result<Self, Error> {
        let mut runs: Vec<Run> = Vec::new();
        for (n, word) in words.iter().enumerate().take(usize::from(u16::MAX)) {
            let n = clamp16(n);
            let add = word.text.len() + 1;
            match runs.last_mut() {
                Some(run)
                    if run.section == word.start.section
                        && usize::from(run.len) + add <= stage::DECORATE_RUN_TEXT_MAX_BYTES =>
                {
                    run.count += 1;
                    run.len += clamp16(add);
                }
                _ => {
                    runs.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    runs.push(Run {
                        section: word.start.section,
                        offset: word.start.offset,
                        first: n,
                        count: 1,
                        len: clamp16(word.text.len().min(stage::DECORATE_RUN_TEXT_MAX_BYTES)),
                    });
                }
            }
        }
        Ok(Self {
            runs,
            ..Self::default()
        })
    }

    /// The page has no words to ask about.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// The fires that ask the whole page, from run `from` on: whole runs, at
    /// most [`RUNS_PER_FIRE`], within the stage's input bytes. `None` when every
    /// run is asked.
    ///
    /// # Errors
    /// `NoMemory`; `Invalid` where the codec refuses (a path over 512 bytes).
    pub fn fire(
        &self,
        words: &[WordBox],
        (path, language): (&str, &str),
        from: usize,
    ) -> Result<Option<Fire>, Error> {
        if from >= self.runs.len() {
            return Ok(None);
        }
        let language = cut(language, stage::DECORATE_PAGE_LANGUAGE_MAX_BYTES);
        let mut room = stage::MAX_INPUT.saturating_sub(PAGE_FRAME + path.len() + language.len());
        let mut end = from;
        while end < self.runs.len() && end - from < RUNS_PER_FIRE {
            let need = usize::from(self.runs[end].len) + RUN_FRAME;
            if need > room {
                break;
            }
            room -= need;
            end += 1;
        }
        if end == from {
            return Err(Error::Invalid);
        }
        let mut texts = String::new();
        let mut bounds: Vec<(usize, usize)> = Vec::new();
        bounds
            .try_reserve_exact(end - from)
            .map_err(|_| Error::NoMemory)?;
        for run in &self.runs[from..end] {
            let start = texts.len();
            run_text(words, run, &mut texts)?;
            let mut stop = texts.len().min(start + stage::DECORATE_RUN_TEXT_MAX_BYTES);
            while !texts.is_char_boundary(stop) {
                stop -= 1;
            }
            bounds.push((start, stop));
        }
        let mut records = Vec::new();
        records
            .try_reserve_exact(end - from + 1)
            .map_err(|_| Error::NoMemory)?;
        records.push(stage::Record::DecoratePage(stage::DecoratePage::new(
            path,
            language,
            (end - from) as u32,
        )));
        for (run, (start, stop)) in self.runs[from..end].iter().zip(&bounds) {
            records.push(stage::Record::DecorateRun(stage::DecorateRun::new(
                run.section,
                run.offset,
                texts.get(*start..*stop).unwrap_or_default(),
            )));
        }
        let mut input = Vec::new();
        input
            .try_reserve_exact(stage::MAX_INPUT)
            .map_err(|_| Error::NoMemory)?;
        input.resize(stage::MAX_INPUT, 0);
        let length = stage::input_encode(&records, &mut input).map_err(|_| Error::Invalid)?;
        input.truncate(length);
        Ok(Some(Fire {
            input,
            first_run: from,
            runs: end - from,
        }))
    }

    /// Takes one subscriber's reply to `fire`: its looks checked for its own
    /// roles, its marks checked against the runs sent, kept up to
    /// [`MAX_MARKS`]. A reply the codec refuses drops all of its marks.
    pub fn absorb(&mut self, fire: &Fire, subscriber: &str, payload: &[u8]) {
        let Ok(records) = stage::reply_decode(payload) else {
            return;
        };
        let Ok(sent) = runs_sent(&fire.input) else {
            return;
        };
        // What each look number of this reply is: undefined, defined, or kept
        // as `looks[i]`.
        let mut table = [UNDEFINED; LOOKS];
        for record in records.clone().flatten() {
            if let stage::Record::DecorateLook(look) = record
                && let Some(slot) = table.get_mut(look.look as usize)
                && own_role(look.role, subscriber)
            {
                *slot = DEFINED;
            }
        }
        for record in records.flatten() {
            let stage::Record::DecorateMarks(marks) = record else {
                continue;
            };
            let Some(&state) = table.get(marks.look as usize) else {
                continue;
            };
            if state == UNDEFINED || marks.ranges.len() % MARK_BYTES != 0 {
                continue;
            }
            let (packed, _) = marks.ranges.as_chunks::<MARK_BYTES>();
            for &[r0, r1, f0, f1, t0, t1] in packed {
                let (run, from, to) = (
                    u16::from_le_bytes([r0, r1]),
                    u16::from_le_bytes([f0, f1]),
                    u16::from_le_bytes([t0, t1]),
                );
                let Some(text) = sent.get(usize::from(run)) else {
                    self.dropped += 1;
                    continue;
                };
                let (f, t) = (usize::from(from), usize::from(to));
                if f >= t
                    || t > text.len()
                    || !text.is_char_boundary(f)
                    || !text.is_char_boundary(t)
                {
                    self.dropped += 1;
                    continue;
                }
                if self.marks.len() >= MAX_MARKS {
                    self.dropped += 1;
                    continue;
                }
                let look = match table[marks.look as usize] {
                    DEFINED => match self.keep_look(payload, marks.look) {
                        Some(kept) => {
                            table[marks.look as usize] = kept;
                            kept
                        }
                        None => {
                            self.dropped += 1;
                            continue;
                        }
                    },
                    kept => kept,
                };
                if self.marks.try_reserve(1).is_err() {
                    self.dropped += 1;
                    continue;
                }
                self.marks.push(Mark {
                    run: clamp16(fire.first_run + usize::from(run)),
                    from,
                    to,
                    look,
                });
            }
        }
    }

    /// The look numbered `number` in `payload`, kept; its index.
    fn keep_look(&mut self, payload: &[u8], number: u32) -> Option<u8> {
        let records = stage::reply_decode(payload).ok()?;
        let look = records.flatten().find_map(|record| match record {
            stage::Record::DecorateLook(look) if look.look == number => Some(look),
            _ => None,
        })?;
        let index = u8::try_from(self.looks.len()).ok().filter(|i| *i < KEPT)?;
        let mut role = String::new();
        push(&mut role, look.role).ok()?;
        self.looks.try_reserve(1).ok()?;
        self.looks.push(Look {
            role,
            background: code(look.background),
            ink: code(look.ink),
            line: code(look.line),
            line_tone: code(look.line_tone),
            thickness: code(look.thickness.min(codes::MAX_THICKNESS)),
            flags: code(look.flags & (codes::FRAME | codes::STRUCK | codes::TAPS)),
        });
        Some(index)
    }

    /// Each kept mark with its look and the words it covers, in the order
    /// the subscribers answered.
    pub fn marked<'a>(
        &'a self,
        words: &'a [WordBox],
    ) -> impl DoubleEndedIterator<Item = (&'a Look, Covered<'a>)> + 'a {
        self.marks.iter().filter_map(move |mark| {
            let look = self.looks.get(usize::from(mark.look))?;
            let run = self.runs.get(usize::from(mark.run))?;
            Some((
                look,
                Covered {
                    words,
                    run: *run,
                    from: mark.from,
                    to: mark.to,
                },
            ))
        })
    }

    /// The mark that takes taps under `(x, y)`: its look and the words it
    /// covers. The first subscriber's wins where marks meet.
    #[must_use]
    pub fn tapped<'a>(
        &'a self,
        words: &'a [WordBox],
        x: i32,
        y: i32,
    ) -> Option<(&'a Look, Covered<'a>)> {
        self.marked(words)
            .filter(|(look, _)| u32::from(look.flags) & codes::TAPS != 0)
            .find(|(_, covered)| {
                covered.words().any(|word| {
                    crate::native_wordbox::word_at(core::slice::from_ref(word), x, y).is_some()
                })
            })
    }
}

/// A tapped mark's text and its start and end place (section, offset).
pub type TapText = (String, (u32, u32), (u32, u32));

/// The words a mark covers.
#[derive(Clone, Copy, Debug)]
pub struct Covered<'a> {
    words: &'a [WordBox],
    run: Run,
    from: u16,
    to: u16,
}

impl<'a> Covered<'a> {
    /// The words (word boxes) the mark touches.
    pub fn words(&self) -> impl Iterator<Item = &'a WordBox> + 'a {
        let (from, to) = (self.from, self.to);
        spans(self.words, &self.run)
            .filter(move |(_, start, end)| *start < to && from < *end)
            .map(|(word, _, _)| word)
    }

    /// The decoration input of a tap on it: the text (at most 256 bytes, cut
    /// at a character), and its start and end place.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn tapped_text(&self) -> Result<TapText, Error> {
        let mut text = String::new();
        let mut first = None;
        let mut last = None;
        for word in self.words() {
            let joined = last.is_some_and(|l: &WordBox| l.start == word.start);
            if first.is_some() && !joined {
                push(&mut text, " ")?;
            }
            push(&mut text, word.text.trim_end_matches(['-', '\u{AD}']))?;
            first.get_or_insert(word);
            last = Some(word);
        }
        let keep = cut(&text, stage::DECORATE_TAPPED_TEXT_MAX_BYTES).len();
        text.truncate(keep);
        let (first, last) = (first.ok_or(Error::NotFound)?, last.ok_or(Error::NotFound)?);
        Ok((
            text,
            (first.start.section, first.start.offset),
            (last.end.section, last.end.offset),
        ))
    }
}

const UNDEFINED: u8 = u8::MAX;
const DEFINED: u8 = u8::MAX - 1;
/// The most looks a page keeps (indexes below [`DEFINED`]).
const KEPT: u8 = 64;

fn code(value: u32) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

/// `role` is `<subscriber>.<name>`.
fn own_role(role: &str, subscriber: &str) -> bool {
    role.strip_prefix(subscriber)
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|name| !name.is_empty())
}

/// The run texts a fire's input carries, in order.
fn runs_sent(input: &[u8]) -> Result<Vec<&str>, Error> {
    let records = stage::input_decode(input).map_err(|_| Error::Invalid)?;
    let mut texts = Vec::new();
    texts
        .try_reserve_exact(RUNS_PER_FIRE)
        .map_err(|_| Error::NoMemory)?;
    for record in records.flatten() {
        if let stage::Record::DecorateRun(run) = record {
            texts.push(run.text);
        }
    }
    Ok(texts)
}

/// `text` cut to at most `max` bytes, at a character.
fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The input of a tap on a mark of `look` covering `covered` in the book at
/// `path`.
///
/// # Errors
/// `NoMemory`; `Invalid` where the codec refuses.
pub fn tap_input(look: &Look, covered: &Covered<'_>, path: &str) -> Result<Vec<u8>, Error> {
    let (text, start, end) = covered.tapped_text()?;
    let record = stage::Record::DecorateTapped(stage::DecorateTapped::new(
        &look.role, &text, path, start.0, start.1, end.0, end.1,
    ));
    let mut input = Vec::new();
    input
        .try_reserve_exact(stage::MAX_INPUT)
        .map_err(|_| Error::NoMemory)?;
    input.resize(stage::MAX_INPUT, 0);
    let length = stage::input_encode(&[record], &mut input).map_err(|_| Error::Invalid)?;
    input.truncate(length);
    Ok(input)
}

/// The tap was taken: `subscriber` owns `role` and replied `decorate.taken` 1.
#[must_use]
pub fn taken(role: &str, subscriber: &str, payload: &[u8]) -> bool {
    own_role(role, subscriber)
        && stage::reply_decode(payload).is_ok_and(|mut records| {
            records.any(|r| matches!(r, Ok(stage::Record::DecorateTaken(t)) if t.taken == 1))
        })
}

#[cfg(test)]
mod tests;
