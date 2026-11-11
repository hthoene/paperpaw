use super::*;
use crate::{
    fail_alloc::{exempt, sweep},
    native_typeset::{Faces, Measure, Role},
};
use alloc::{string::ToString, vec};
use paperpaw_sdk::{
    NoStorage, Storage,
    document::{Block, BlockKind, Document, DocumentError, Locator, Metadata},
    layout::{LayoutContext, Style, layout_page},
};

/// Every character is 10 pixels wide at size 20; a line is 25 pixels high.
#[derive(Debug, Default)]
struct Fixed;

impl Measure for Fixed {
    fn width(&self, _: Role, size: f32, text: &str) -> Result<f32, Error> {
        Ok(size * 0.5 * text.chars().count() as f32)
    }
    fn metrics(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        Ok((size * 0.8, size * 0.2))
    }
}

struct Book(Vec<Block>);

impl Document for Book {
    fn metadata(&self) -> &Metadata {
        static EMPTY: std::sync::OnceLock<Metadata> = std::sync::OnceLock::new();
        EMPTY.get_or_init(Metadata::default)
    }
    fn sections(&self) -> u32 {
        1
    }
    fn section_len(&self, _: u32) -> u32 {
        self.0.last().map_or(0, |b| b.end)
    }
    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(self.0.iter().find(|b| b.end > at.offset).cloned())
    }
}

/// The pages of a book of one paragraph, `width` and `lines` as the page has them,
/// laid out ahead.
struct Laid {
    pages: Vec<Page>,
}

fn laid(text: &str, width: f32, lines: u32) -> Laid {
    let block = Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.into());
    let book = Book(vec![block]);
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, width, &style, &faces);
    let mut pages = Vec::new();
    let mut at = Locator::START;
    loop {
        let page = layout_page(&cx, at, 25.0 * lines as f32).unwrap();
        let past = page.is_past_end();
        at = page.end;
        pages.push(page);
        if past || pages.len() > 50 {
            break;
        }
    }
    Laid { pages }
}

impl Pages for Laid {
    fn with_page<R>(&mut self, start: Place, f: impl FnOnce(&Page) -> R) -> Result<R, Error> {
        let page = self
            .pages
            .iter()
            .find(|p| (p.start.section, p.start.offset) == start)
            .ok_or(Error::NotFound)?;
        Ok(f(page))
    }
}

const TEXT: &str = "Alpha beta gamma delta. Epsilon zeta eta theta iota kappa lambda mu nu xi \
omicron pi rho sigma tau. Upsilon phi chi psi omega!";

const SENTENCES: [&str; 3] = [
    "Alpha beta gamma delta.",
    "Epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau.",
    "Upsilon phi chi psi omega!",
];

/// 200 pixels are 20 letters, 3 lines a page: the second sentence goes over pages.
fn small() -> Laid {
    laid(TEXT, 200.0, 3)
}

fn start_of(laid: &Laid, n: usize) -> Place {
    let page = &laid.pages[n];
    (page.start.section, page.start.offset)
}

#[test]
fn the_words_of_a_page_are_numbered_by_reading_order() {
    let laid = laid("one two three", 400.0, 4);
    let found = words(&laid.pages[0]).unwrap();
    let texts: Vec<&str> = found.iter().map(|w| w.text).collect();
    assert_eq!(texts, ["one", "two", "three"]);
    assert!(found[0].new_line && !found[1].new_line);
    assert_eq!(found[1].at, (0, 4));
    assert_eq!(found[2].at, (0, 8));
    // A page past the end has none.
    let last = laid.pages.last().unwrap();
    assert!(words(last).unwrap().is_empty());
}

#[test]
fn a_sentence_that_goes_on_on_the_next_page_is_spoken_whole() {
    let mut pages = small();
    assert!(pages.pages.len() > 3, "{}", pages.pages.len());
    let start = start_of(&pages, 0);
    let (first, text) = sentence(&mut pages, start, 0, true, None).unwrap().unwrap();
    assert_eq!(text, SENTENCES[0]);
    assert_eq!(first.from, 0);
    assert_eq!(first.carried, 0);
    // The second is followed on its page, and goes on over the end of the page.
    let (second, text) = following(&mut pages, &first, true).unwrap().unwrap();
    assert_eq!(text, SENTENCES[1]);
    assert_eq!(second.at, (0, 24));
    // The last: on the page it is, after the one that ended the other.
    let (third, text) = following(&mut pages, &second, true).unwrap().unwrap();
    assert_eq!(text, SENTENCES[2]);
    assert!(following(&mut pages, &third, true).unwrap().is_none());
}

#[test]
fn without_turning_the_sentence_ends_with_its_page() {
    let mut pages = small();
    let start = start_of(&pages, 0);
    let mut spoken = Vec::new();
    let mut at = sentence(&mut pages, start, 0, false, None)
        .unwrap()
        .unwrap();
    loop {
        spoken.push(at.1.clone());
        match following(&mut pages, &at.0, false).unwrap() {
            Some(next) => at = next,
            None => break,
        }
    }
    // Only what is on the first page, the last sentence cut by its end.
    assert_eq!(spoken[0], SENTENCES[0]);
    assert!(spoken.len() < 4);
    let on_page: usize = words(&pages.pages[0]).unwrap().len();
    let said: usize = spoken.iter().map(|s| s.split(' ').count()).sum();
    assert_eq!(said, on_page);
}

#[test]
fn a_text_over_the_hosts_limit_is_cut_after_a_word() {
    let mut text = "wörd ".repeat(600);
    text.pop();
    cut(&mut text);
    assert!(text.len() <= MAX_TEXT);
    assert!(text.ends_with("wörd") && text.len() > MAX_TEXT - 8);
    let mut short = String::from("short text");
    cut(&mut short);
    assert_eq!(short, "short text");
    let mut no_space = "x".repeat(MAX_TEXT + 10);
    cut(&mut no_space);
    assert_eq!(no_space.len(), MAX_TEXT);
}

#[test]
fn the_text_limit_is_the_hosts() {
    let text = "x".repeat(MAX_TEXT);
    assert!(paperpaw_sdk::native::ui::SpeechRequest::new(1, &text).is_ok());
    let more = "x".repeat(MAX_TEXT + 1);
    assert!(paperpaw_sdk::native::ui::SpeechRequest::new(1, &more).is_err());
}

#[test]
fn a_word_is_found_by_its_place() {
    let mut pages = small();
    let first = start_of(&pages, 0);
    let page = &pages.pages[0];
    let all = words(page).unwrap();
    let third = all[2].at;
    assert_eq!(word_at(page, third).unwrap(), Some(2));
    // Between two words: the next one.
    assert_eq!(word_at(page, (0, third.1 + 1)).unwrap(), Some(3));
    // Not on this page.
    assert_eq!(word_at(page, (0, 100_000)).unwrap(), None);
    assert_eq!(word_at(&pages.pages[1], (0, 0)).unwrap(), None);
    let _ = (&mut pages, first);
}

#[test]
fn every_refused_allocation_is_no_memory() {
    let mut pages = exempt(small);
    let start = exempt(|| start_of(&pages, 0));
    sweep(|| sentence(&mut pages, start, 0, true, None).map(|s| s.map(|(_, t)| t.len())));
    let (first, _) = exempt(|| sentence(&mut pages, start, 0, true, None).unwrap().unwrap());
    sweep(|| following(&mut pages, &first, true).map(|s| s.map(|(_, t)| t.len())));
    sweep(|| words(&pages.pages[0]).map(|w| w.len()));
}

/// What reading aloud did to the reading screen, recorded.
struct Fake {
    pages: Laid,
    shown: Place,
    options: Options,
    now: u64,
    spoken: Vec<(String, u32, bool)>,
    says: Vec<&'static str>,
    marks: Vec<Option<Range<usize>>>,
    stops: usize,
    refuse_speech: Option<Error>,
}

impl Fake {
    fn new(pages: Laid) -> Self {
        let shown = start_of(&pages, 0);
        Self {
            pages,
            shown,
            options: Options {
                turn: true,
                highlight: true,
                minutes: None,
            },
            now: 1_000,
            spoken: Vec::new(),
            says: Vec::new(),
            marks: Vec::new(),
            stops: 0,
            refuse_speech: None,
        }
    }
}

impl Pages for Fake {
    fn with_page<R>(&mut self, start: Place, f: impl FnOnce(&Page) -> R) -> Result<R, Error> {
        self.pages.with_page(start, f)
    }
}

impl Scene for Fake {
    fn shown(&self) -> Place {
        self.shown
    }
    fn turn_to(&mut self, start: Place) -> Result<bool, Error> {
        let end = self
            .pages
            .with_page(self.shown, |p| (p.end.section, p.end.offset))?;
        if end != start {
            return Ok(false);
        }
        self.shown = start;
        Ok(true)
    }
    fn speak(&mut self, text: &str, cookie: u32, queue: bool) -> Result<(), Error> {
        if let Some(error) = self.refuse_speech {
            return Err(error);
        }
        self.spoken.push((text.to_string(), cookie, queue));
        Ok(())
    }
    fn stop(&mut self) {
        self.stops += 1;
    }
    fn mark(&mut self, words: Option<Range<usize>>) {
        self.marks.push(words);
    }
    fn say(&mut self, key: &'static str) {
        self.says.push(key);
    }
    fn options(&self) -> Options {
        self.options
    }
    fn now_us(&self) -> Option<u64> {
        Some(self.now)
    }
}

/// The host speaks what is handed over, in order, one after the other: each
/// answer is for the sentence that was spoken.
fn speak_all(speaker: &mut Speaker, scene: &mut Fake) -> Vec<String> {
    let mut said = Vec::new();
    for _ in 0..20 {
        let Some(now) = speaker.aloud.map(|a| a.now.cookie) else {
            break;
        };
        let text = scene.spoken.iter().find(|s| s.1 == now).unwrap().0.clone();
        said.push(text);
        speaker.spoken(scene, now, SpeechStatus::Spoken);
    }
    said
}

#[test]
fn reading_aloud_says_the_sentences_one_after_another_and_turns_the_pages() {
    let mut scene = Fake::new(small());
    let mut speaker = Speaker::new(7);
    speaker.start(&mut scene);
    assert_eq!(scene.says, ["reader.aloud.started"]);
    // The first is spoken at once (replacing what was), the second is queued.
    assert_eq!(scene.spoken.len(), 2);
    assert!(!scene.spoken[0].2 && scene.spoken[1].2);
    assert_eq!(scene.spoken[0].0, SENTENCES[0]);
    assert_eq!(scene.spoken[1].0, SENTENCES[1]);
    // Every cookie is this screen's.
    assert!(scene.spoken.iter().all(|s| s.1 >> 16 == 7));
    assert!(speaker.owns(scene.spoken[0].1) && speaker.owns(scene.spoken[1].1));
    let said = speak_all(&mut speaker, &mut scene);
    assert_eq!(said, SENTENCES);
    assert!(!speaker.active());
    // The pages turned with it, and the underline went with each sentence and ended.
    assert!(scene.shown != start_of(&scene.pages, 0));
    assert_eq!(scene.marks.last(), Some(&None));
    assert!(scene.marks.iter().flatten().count() >= 3);
    assert!(scene.says.len() == 1, "{:?}", scene.says);
}

#[test]
fn without_the_underline_nothing_is_marked() {
    let mut scene = Fake::new(small());
    scene.options.highlight = false;
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    speak_all(&mut speaker, &mut scene);
    assert!(scene.marks.iter().all(Option::is_none));
}

#[test]
fn without_turning_pages_it_stops_at_the_end_of_the_page() {
    let mut scene = Fake::new(small());
    scene.options.turn = false;
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    speak_all(&mut speaker, &mut scene);
    assert!(!speaker.active());
    assert_eq!(scene.shown, start_of(&scene.pages, 0));
    assert_eq!(scene.says.last(), Some(&"reader.aloud.page-done"));
}

#[test]
fn a_pause_stops_the_speech_and_it_goes_on_from_there() {
    let mut scene = Fake::new(small());
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    let first = speaker.aloud.unwrap().now.cookie;
    speaker.spoken(&mut scene, first, SpeechStatus::Spoken);
    // The second sentence is spoken now.
    speaker.pause(&mut scene, true);
    assert!(!speaker.active());
    assert_eq!(scene.stops, 1);
    assert_eq!(scene.says.last(), Some(&"reader.aloud.paused"));
    scene.spoken.clear();
    // The page on show is the second sentence's: going on speaks it again.
    speaker.start(&mut scene);
    assert_eq!(scene.spoken[0].0, SENTENCES[1]);
    assert!(!scene.spoken[0].2);
}

#[test]
fn an_answer_of_another_time_is_not_heard() {
    let mut scene = Fake::new(small());
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    let spoken = scene.spoken.len();
    // Of the sentence after the one now: not yet.
    let next = scene.spoken[1].1;
    speaker.spoken(&mut scene, next, SpeechStatus::Spoken);
    speaker.spoken(&mut scene, 0xdead_beef, SpeechStatus::NoVoice);
    assert!(speaker.active());
    assert_eq!(scene.spoken.len(), spoken);
    // Nothing is read: an answer is not heard.
    let mut idle = Speaker::new(1);
    idle.spoken(&mut scene, 1, SpeechStatus::Spoken);
    assert!(!idle.active());
}

#[test]
fn a_voice_that_is_missing_or_a_device_that_cannot_speak_ends_it_with_a_notice() {
    for (status, notice) in [
        (SpeechStatus::NoVoice, "reader.aloud.failed"),
        (SpeechStatus::Unsupported, "reader.aloud.unavailable"),
    ] {
        let mut scene = Fake::new(small());
        let mut speaker = Speaker::new(1);
        speaker.start(&mut scene);
        let now = speaker.aloud.unwrap().now.cookie;
        speaker.spoken(&mut scene, now, status);
        assert!(!speaker.active());
        assert_eq!(scene.says.last(), Some(&notice));
        assert_eq!(scene.stops, 1);
        // It does not go on from there: reading aloud starts at the page's start.
        assert!(speaker.resume.is_none());
    }
}

#[test]
fn speech_replaced_by_another_voice_is_a_pause() {
    let mut scene = Fake::new(small());
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    let now = speaker.aloud.unwrap().now.cookie;
    speaker.spoken(&mut scene, now, SpeechStatus::Stopped);
    assert!(!speaker.active() && speaker.resume.is_some());
}

#[test]
fn a_host_that_refuses_to_speak_says_so() {
    for (error, notice) in [
        (Error::Unsupported, "reader.aloud.unavailable"),
        (Error::NoMemory, "reader.no-memory"),
        (Error::Busy, "reader.aloud.failed"),
    ] {
        let mut scene = Fake::new(small());
        scene.refuse_speech = Some(error);
        let mut speaker = Speaker::new(1);
        speaker.start(&mut scene);
        assert!(!speaker.active());
        assert_eq!(scene.says.last(), Some(&notice));
    }
}

#[test]
fn the_timer_stops_it_after_the_sentence_that_ran_over() {
    let mut scene = Fake::new(small());
    scene.options.minutes = Some(15);
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    let until = speaker.aloud.unwrap().until_us.unwrap();
    assert_eq!(until, 1_000 + 15 * 60_000_000);
    let now = speaker.aloud.unwrap().now.cookie;
    // Before the time: it goes on.
    scene.now = until - 1;
    speaker.spoken(&mut scene, now, SpeechStatus::Spoken);
    assert!(speaker.active());
    let now = speaker.aloud.unwrap().now.cookie;
    scene.now = until;
    speaker.spoken(&mut scene, now, SpeechStatus::Spoken);
    assert!(!speaker.active());
    assert_eq!(
        &scene.says[scene.says.len() - 1..],
        ["reader.aloud.timer-done"]
    );
    assert!(!scene.says.contains(&"reader.aloud.paused"));
    // It can be started again from there (a new timer).
    assert!(speaker.resume.is_some());
}

#[test]
fn closing_or_leaving_ends_it_for_good() {
    let mut scene = Fake::new(small());
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    speaker.stop(&mut scene);
    assert!(!speaker.active() && speaker.resume.is_none());
    assert_eq!(scene.stops, 1);
    assert_eq!(scene.marks.last(), Some(&None));
    // Nothing to end: nothing said to the host.
    speaker.stop(&mut scene);
    assert_eq!(scene.stops, 1);
}

#[test]
fn a_page_without_words_is_not_read() {
    let mut scene = Fake::new(laid("", 200.0, 2));
    let mut speaker = Speaker::new(1);
    speaker.start(&mut scene);
    assert!(!speaker.active() && scene.spoken.is_empty());
    assert_eq!(scene.says.last(), Some(&"reader.aloud.page-done"));
}

#[test]
fn only_actions_and_gestures_pause() {
    use paperpaw_sdk::native::callbacks::{Input, InputGestureKind, KeyState};
    let point = paperpaw_sdk::Point::new(1, 1);
    assert!(pauses(&Input::Action {
        now_ms: 0,
        action: "core.next-page"
    }));
    assert!(pauses(&Input::Gesture {
        now_ms: 0,
        position: point,
        kind: InputGestureKind::Tap
    }));
    assert!(!pauses(&Input::Key {
        now_ms: 0,
        key: "x",
        state: KeyState::Press
    }));
}
