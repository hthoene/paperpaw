//! Reading aloud on interface 7, without the host: the words of a page, the
//! sentence that is spoken and the one handed over to follow it, and what
//! happens when a sentence was spoken, the reader was touched or the timer
//! ran out. The same behaviour as Beta 3's `screen/hands_free.rs`; what it
//! needs of the reading screen is the [`Scene`], which `native_view/aloud.rs`
//! gives it with the host's speech commands (`docs/abi/ui.md` section 7) and
//! tests give with a recording fake. Allocation is fallible; nothing is held
//! of a page but the words of one at a time (borrowed, 20 bytes each).
use crate::native_aloud::{ends_sentence, sentence_end_by, spoken_into};
use alloc::{string::String, vec::Vec};
use core::ops::Range;
use paperpaw_sdk::{layout::Page, native::Error, native::events::SpeechStatus};

/// A place in the book: the section and the offset in it.
pub type Place = (u32, u32);

/// The most bytes the host speaks at once (`command.speak.text`): a sentence
/// that goes on over a page and is longer is cut after a word.
pub const MAX_TEXT: usize = 2048;

/// Most empty pages (pictures only) passed to find the next words.
const MAX_SKIPS: usize = 8;

/// A word of a page as it is spoken and where it is in the book.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word<'a> {
    /// The word as the page shows it (a hyphenated one in parts).
    pub text: &'a str,
    /// It is the first of its line.
    pub new_line: bool,
    /// Where it starts in the book (the section and the offset in it).
    pub at: Place,
}

/// The words of `page` in reading order, one for each stretch of letters
/// between spaces (a word that goes over two runs is one; the lines a filter
/// inserted have no words of the book's). The numbers are the ones
/// `native_draw::draw_lines_marked` marks by.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn words(page: &Page) -> Result<Vec<Word<'_>>, Error> {
    let of = |text: &'_ str| text.split(' ').filter(|w| !w.is_empty()).count();
    let count = page
        .lines
        .iter()
        .filter(|l| !l.inserted)
        .map(|l| of(&l.text))
        .sum();
    let mut found = Vec::new();
    found
        .try_reserve_exact(count)
        .map_err(|_| Error::NoMemory)?;
    for line in page.lines.iter().filter(|l| !l.inserted) {
        let pieces = line.text.split(' ').filter(|w| !w.is_empty());
        for (n, text) in pieces.enumerate() {
            let start = line.words.get(n).map_or(0, |w| w.0);
            found.push(Word {
                text,
                new_line: n == 0,
                at: (line.section, start),
            });
        }
    }
    Ok(found)
}

/// A sentence read aloud, by the words of the page that starts at `page`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sentence {
    /// Where its page starts.
    pub page: Place,
    /// Its first word and the one after its last, on that page.
    pub from: usize,
    pub to: usize,
    /// The words of its page.
    pub words: usize,
    /// Words of the following page spoken with it (it goes on there).
    pub carried: usize,
    /// Where its first word is in the book.
    pub at: Place,
}

impl Sentence {
    /// The words of its page to underline.
    #[must_use]
    pub const fn range(&self) -> Range<usize> {
        self.from..self.to
    }
}

/// The pages of the book as the reading screen has them.
pub trait Pages {
    /// Runs `f` on the page that starts at `start`: the page on show as it is,
    /// any other laid out for the call.
    ///
    /// # Errors
    /// What the layout refuses.
    fn with_page<R>(&mut self, start: Place, f: impl FnOnce(&Page) -> R) -> Result<R, Error>;
}

/// What a sentence is made of on its page, before the rest of it on the next.
struct First {
    sentence: Sentence,
    text: String,
    /// The page ends in the middle of the sentence.
    goes_on: bool,
    /// Where the next page starts.
    end: Place,
}

fn place(page: &Page) -> (Place, Place) {
    (
        (page.start.section, page.start.offset),
        (page.end.section, page.end.offset),
    )
}

/// The sentence that starts at word `from` of `page`: none for a page past the
/// end or the one of `guard` (it would not move on); an empty text for a page
/// the words before `from` used up.
fn on_page(page: &Page, from: usize, guard: Option<Place>) -> Result<Option<First>, Error> {
    let (start, end) = place(page);
    if page.is_past_end() || Some(start) == guard {
        return Ok(None);
    }
    let words = words(page)?;
    let count = words.len();
    let from = from.min(count);
    let to = if from < count {
        sentence_end_by(count, from, |i| {
            words.get(i).is_some_and(|w| ends_sentence(w.text))
        })
    } else {
        from
    };
    let mut text = String::new();
    let picked = words.get(from..to).unwrap_or_default();
    spoken_into(&mut text, picked.iter().map(|w| (w.text, w.new_line)))?;
    let goes_on = to > from && to == count && words.last().is_some_and(|w| !ends_sentence(w.text));
    Ok(Some(First {
        sentence: Sentence {
            page: start,
            from,
            to,
            words: count,
            carried: 0,
            at: words.get(from).map_or(start, |w| w.at),
        },
        text,
        goes_on,
        end,
    }))
}

/// The first sentence of the next page added to `text`, as the number of its
/// words; nothing is added where it fails.
fn carry(next: &Page, text: &mut String) -> Result<usize, Error> {
    if next.is_past_end() {
        return Ok(0);
    }
    let words = words(next)?;
    let carried = sentence_end_by(words.len(), 0, |i| {
        words.get(i).is_some_and(|w| ends_sentence(w.text))
    });
    let before = text.len();
    let picked = words.get(..carried).unwrap_or_default();
    match spoken_into(text, picked.iter().map(|w| (w.text, w.new_line))) {
        Ok(()) => Ok(carried),
        Err(error) => {
            text.truncate(before);
            Err(error)
        }
    }
}

/// `text` cut after a word where it is over [`MAX_TEXT`] bytes.
fn cut(text: &mut String) {
    if text.len() <= MAX_TEXT {
        return;
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let at = text
        .get(..end)
        .and_then(|t| t.rfind(' '))
        .filter(|&i| i > 0)
        .unwrap_or(end);
    text.truncate(at);
}

/// The sentence that starts at word `from` of the page that starts at
/// `start`, and its text as it is spoken; with `turn`, one that goes on on the
/// next page is spoken whole. `None` for a page past the end.
///
/// # Errors
/// [`Error::NoMemory`] and what the layout of the page refuses.
pub fn sentence<P: Pages>(
    pages: &mut P,
    start: Place,
    from: usize,
    turn: bool,
    guard: Option<Place>,
) -> Result<Option<(Sentence, String)>, Error> {
    let Some(mut first) = pages.with_page(start, |page| on_page(page, from, guard))?? else {
        return Ok(None);
    };
    if first.goes_on && turn && first.end != start {
        // A next page that cannot be laid out leaves the sentence unfinished.
        let text = &mut first.text;
        if let Ok(Ok(carried)) = pages.with_page(first.end, |next| carry(next, text)) {
            first.sentence.carried = carried;
        }
    }
    cut(&mut first.text);
    Ok(Some((first.sentence, first.text)))
}

/// The sentence after `after` on its page, else (with `turn`) on the next
/// one. A sentence with no words (a page of a picture) is passed over; `None`
/// at the end of the book, or of the page without `turn`.
///
/// # Errors
/// As [`sentence`].
pub fn following<P: Pages>(
    pages: &mut P,
    after: &Sentence,
    turn: bool,
) -> Result<Option<(Sentence, String)>, Error> {
    let mut at = *after;
    for _ in 0..MAX_SKIPS {
        let found = if at.to < at.words {
            sentence(pages, at.page, at.to, turn, None)?
        } else if turn {
            let (_, end) = pages.with_page(at.page, place)?;
            sentence(pages, end, at.carried, turn, Some(at.page))?
        } else {
            None
        };
        match found {
            Some((next, text)) if text.is_empty() => at = next,
            other => return Ok(other),
        }
    }
    Ok(None)
}

/// The first sentence to speak from word `from` of the page on show; it may be
/// on a later page when the words of this one from there are none.
///
/// # Errors
/// As [`sentence`].
pub fn first_from<P: Pages>(
    pages: &mut P,
    start: Place,
    from: usize,
    turn: bool,
) -> Result<Option<(Sentence, String)>, Error> {
    match sentence(pages, start, from, turn, None)? {
        Some((s, text)) if text.is_empty() => following(pages, &s, turn),
        other => Ok(other),
    }
}

/// The number of the word of `page` that starts at `place` or after it, if
/// `place` is on the page.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn word_at(page: &Page, at: Place) -> Result<Option<usize>, Error> {
    let (start, end) = place(page);
    if at < start || at >= end {
        return Ok(None);
    }
    Ok(words(page)?.iter().position(|w| w.at >= at))
}

/// The reader's settings reading aloud follows, read when they are needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Pages are turned (else it stops at the end of the page).
    pub turn: bool,
    /// The sentence is underlined.
    pub highlight: bool,
    /// Minutes until reading aloud stops by itself.
    pub minutes: Option<u64>,
}

/// What reading aloud does with the reading screen and the host.
pub trait Scene: Pages {
    /// Where the page on show starts.
    fn shown(&self) -> Place;
    /// Shows the page that starts at `start`, which must be the one after
    /// the page on show; whether it is on show.
    ///
    /// # Errors
    /// What the layout or a redraw refuses.
    fn turn_to(&mut self, start: Place) -> Result<bool, Error>;
    /// Hands `text` over to be spoken, after what is spoken with `queue`;
    /// `cookie` comes back with the answer.
    ///
    /// # Errors
    /// The host's refusal.
    fn speak(&mut self, text: &str, cookie: u32, queue: bool) -> Result<(), Error>;
    /// Stops what is spoken.
    fn stop(&mut self);
    /// Underlines these words of the page on show; none ends it.
    fn mark(&mut self, words: Option<Range<usize>>);
    /// Says `key` to the reader (a notice over the page).
    fn say(&mut self, key: &'static str);
    /// The settings of reading aloud.
    fn options(&self) -> Options;
    /// The host's clock, in microseconds, where it has one.
    fn now_us(&self) -> Option<u64>;
}

/// A sentence spoken with the cookie its answer carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spoken {
    sentence: Sentence,
    cookie: u32,
}

/// Reading aloud: the sentence spoken and the one handed over to follow it
/// without a pause.
#[derive(Clone, Copy, Debug)]
struct Aloud {
    now: Spoken,
    next: Option<Spoken>,
    /// When the timer stops reading aloud.
    until_us: Option<u64>,
}

/// One reading screen's reading aloud.
#[derive(Debug, Default)]
pub struct Speaker {
    aloud: Option<Aloud>,
    /// Where it was paused: the first word of the sentence.
    resume: Option<Place>,
    /// The upper half of every cookie: this screen's.
    base: u32,
    seq: u16,
}

/// Whether an input of the reader pauses reading aloud: an action (a button
/// or a tap zone as the host maps it) or a gesture; not a raw key (its action
/// follows it) nor a finger's moves.
#[must_use]
pub fn pauses(input: &paperpaw_sdk::native::callbacks::Input<'_>) -> bool {
    use paperpaw_sdk::native::callbacks::Input;
    matches!(input, Input::Action { .. } | Input::Gesture { .. })
}

impl Speaker {
    /// Reading aloud of the screen `instance`: its cookies are its own.
    #[must_use]
    pub const fn new(instance: u32) -> Self {
        Self {
            aloud: None,
            resume: None,
            base: instance << 16,
            seq: 0,
        }
    }

    /// Whether the book is being read aloud.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.aloud.is_some()
    }

    /// Whether `cookie` is of a sentence handed over by this screen.
    #[must_use]
    pub const fn owns(&self, cookie: u32) -> bool {
        match &self.aloud {
            Some(a) => a.now.cookie == cookie || matches!(&a.next, Some(n) if n.cookie == cookie),
            None => false,
        }
    }

    fn cookie(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.base | u32::from(self.seq)
    }

    /// Starts reading aloud on the page on show, from where it was paused if
    /// that is on this page.
    pub fn start<S: Scene>(&mut self, scene: &mut S) {
        let options = scene.options();
        let shown = scene.shown();
        let from = match self.resume.take() {
            Some(at) => scene
                .with_page(shown, |page| word_at(page, at))
                .ok()
                .and_then(Result::ok)
                .flatten()
                .unwrap_or(0),
            None => 0,
        };
        let until = match (options.minutes, scene.now_us()) {
            (Some(m), Some(now)) => Some(now.saturating_add(m.saturating_mul(60_000_000))),
            _ => None,
        };
        self.begin(scene, from, until);
        // Said once the first sentence was taken: a device that cannot speak
        // says so, and not "started" first.
        if self.aloud.is_some() {
            scene.say("reader.aloud.started");
        }
    }

    /// Speaks from word `from` of the page on show, handing over the next
    /// sentence at once.
    fn begin<S: Scene>(&mut self, scene: &mut S, from: usize, until_us: Option<u64>) {
        let options = scene.options();
        let shown = scene.shown();
        let first = match first_from(scene, shown, from, options.turn) {
            Ok(Some(first)) => first,
            Ok(None) => return scene.say("reader.aloud.page-done"),
            Err(error) => return fail(scene, error),
        };
        if first.0.page != scene.shown() && !matches!(scene.turn_to(first.0.page), Ok(true)) {
            return scene.say("reader.aloud.failed");
        }
        let now = Spoken {
            sentence: first.0,
            cookie: self.cookie(),
        };
        if let Err(error) = scene.speak(&first.1, now.cookie, false) {
            return fail(scene, error);
        }
        let next = self.hand_over(scene, &now.sentence, options.turn);
        self.aloud = Some(Aloud {
            now,
            next,
            until_us,
        });
        mark(scene, options, &now.sentence);
    }

    /// Hands the sentence after `after` over to be spoken after the one now.
    fn hand_over<S: Scene>(
        &mut self,
        scene: &mut S,
        after: &Sentence,
        turn: bool,
    ) -> Option<Spoken> {
        let (sentence, text) = following(scene, after, turn).ok().flatten()?;
        let cookie = self.cookie();
        scene.speak(&text, cookie, true).ok()?;
        Some(Spoken { sentence, cookie })
    }

    /// Pauses reading aloud; reading aloud again goes on from there.
    pub fn pause<S: Scene>(&mut self, scene: &mut S, notice: bool) {
        let Some(aloud) = self.aloud.take() else {
            return;
        };
        scene.stop();
        self.resume = Some(aloud.now.sentence.at);
        scene.mark(None);
        if notice {
            scene.say("reader.aloud.paused");
        }
    }

    /// Ends reading aloud for good: the book closed or the screen was left.
    pub fn stop<S: Scene>(&mut self, scene: &mut S) {
        if self.aloud.take().is_some() {
            scene.stop();
            scene.mark(None);
        }
        self.resume = None;
    }

    /// A sentence was spoken (or not): the next one is being spoken already;
    /// the one after it is handed over, turning the page where needed. Ends
    /// at the end of the book, of the page if pages are not turned, or with
    /// the timer. An answer to a sentence of another time is not heard.
    pub fn spoken<S: Scene>(&mut self, scene: &mut S, cookie: u32, status: SpeechStatus) {
        let Some(aloud) = self.aloud else {
            return;
        };
        if cookie != aloud.now.cookie {
            return;
        }
        match status {
            SpeechStatus::Spoken => {}
            // Another voice replaced it: paused where it was.
            SpeechStatus::Stopped => return self.pause(scene, true),
            other => {
                self.stop(scene);
                let none = matches!(other, SpeechStatus::Unsupported);
                return scene.say(if none {
                    "reader.aloud.unavailable"
                } else {
                    "reader.aloud.failed"
                });
            }
        }
        if aloud
            .until_us
            .is_some_and(|until| scene.now_us().is_some_and(|now| now >= until))
        {
            self.pause(scene, false);
            return scene.say("reader.aloud.timer-done");
        }
        self.go_on(scene, aloud);
    }

    fn go_on<S: Scene>(&mut self, scene: &mut S, aloud: Aloud) {
        let options = scene.options();
        let Some(next) = aloud.next else {
            // The end of the book, or of the page.
            self.aloud = None;
            scene.mark(None);
            if !options.turn {
                scene.say("reader.aloud.page-done");
            }
            return;
        };
        if next.sentence.page != scene.shown()
            && !matches!(scene.turn_to(next.sentence.page), Ok(true))
        {
            // The page is not where it was meant to be: the layout changed.
            self.aloud = Some(aloud);
            return self.pause(scene, true);
        }
        let after = self.hand_over(scene, &next.sentence, options.turn);
        self.aloud = Some(Aloud {
            now: next,
            next: after,
            until_us: aloud.until_us,
        });
        mark(scene, options, &next.sentence);
    }
}

/// Underlines `sentence` where the setting says so.
fn mark<S: Scene>(scene: &mut S, options: Options, sentence: &Sentence) {
    scene.mark(options.highlight.then(|| sentence.range()));
}

/// What failed reading aloud, said to the reader.
fn fail<S: Scene>(scene: &mut S, error: Error) {
    scene.say(match error {
        Error::NoMemory => "reader.no-memory",
        Error::Unsupported => "reader.aloud.unavailable",
        _ => "reader.aloud.failed",
    });
}

#[cfg(test)]
mod tests;
