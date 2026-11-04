//! The vocabulary's widgets for the home screen (or any screen showing
//! widgets), each in the shared look of widgets (its frame, size and
//! alignment) with settings of its own for what it shows:
//!
//! - *Word of the day* (`vocabulary.widget.word`): a word to go over,
//!   another one each day (or the newest; without a clock, another one
//!   from time to time), with its sentence and book;
//! - *Words saved* (`vocabulary.widget.saved`): how many words are kept,
//!   from how many books and in how many languages, and the newest.
//!
//! - *Cards due* (`vocabulary.widget.due`): how many flash cards are due
//!   now, new ones to show today counted (see [`cards`](crate::cards)).
//!
//! A tap on either of the first two opens the list of words, on the
//! third the flash cards. They read the words when
//! they are updated (when shown and after events), never as they are
//! drawn: one pass over the file, keeping a word.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetLook, WidgetTarget, Wrapped,
};
use paperpaw_sdk::{
    Arg, Canvas, Command, Env, FontHandle, Id, Rect, RegisterError, Registrar, SettingSpec,
};

use crate::OPEN;
use crate::store::{self, Summary, Word};

/// The word of the day.
pub(crate) const WORD: Id = Id::new_static(WORD_NAME);
const WORD_NAME: &str = "vocabulary.widget.word";
/// The words saved.
pub(crate) const SAVED: Id = Id::new_static(SAVED_NAME);
const SAVED_NAME: &str = "vocabulary.widget.saved";
/// The cards due.
pub(crate) const DUE: Id = Id::new_static(DUE_NAME);
const DUE_NAME: &str = "vocabulary.widget.due";
/// Which word the word of the day is: another each day, or the newest.
const PICKS: [&str; 2] = ["vocabulary.pick.daily", "vocabulary.pick.newest"];

/// The word of the day's settings, kept in the program.
static WORD_SETTINGS: [SettingSpec; 13] = [
    widgets::frame_spec("vocabulary.widget.word.frame", WORD, Frame::None),
    widgets::padding_spec("vocabulary.widget.word.padding", WORD, Padding::Auto),
    widgets::paper_spec("vocabulary.widget.word.paper", WORD),
    widgets::ink_spec("vocabulary.widget.word.ink", WORD),
    widgets::corners_spec("vocabulary.widget.word.corners", WORD),
    widgets::frame_line_spec("vocabulary.widget.word.frame-line", WORD),
    widgets::shadow_spec("vocabulary.widget.word.drop-shadow", WORD),
    widgets::size_spec("vocabulary.widget.word.size", WORD, TextSize::Medium, true),
    widgets::align_spec("vocabulary.widget.word.align", WORD, Align::Left),
    SettingSpec::choice(
        "vocabulary.widget.word.pick",
        "vocabulary.setting.pick",
        &PICKS,
        PICKS[0],
        WORD,
    )
    .description("vocabulary.setting.pick.help"),
    widgets::part_spec(
        "vocabulary.widget.word.caption",
        WORD,
        "vocabulary.part.caption",
        true,
    ),
    widgets::part_spec(
        "vocabulary.widget.word.sentence",
        WORD,
        "vocabulary.part.sentence",
        true,
    ),
    widgets::part_spec(
        "vocabulary.widget.word.book",
        WORD,
        "vocabulary.part.book",
        false,
    ),
];

/// The words saved's settings, kept in the program.
static SAVED_SETTINGS: [SettingSpec; 13] = [
    widgets::frame_spec("vocabulary.widget.saved.frame", SAVED, Frame::None),
    widgets::padding_spec("vocabulary.widget.saved.padding", SAVED, Padding::Auto),
    widgets::paper_spec("vocabulary.widget.saved.paper", SAVED),
    widgets::ink_spec("vocabulary.widget.saved.ink", SAVED),
    widgets::corners_spec("vocabulary.widget.saved.corners", SAVED),
    widgets::frame_line_spec("vocabulary.widget.saved.frame-line", SAVED),
    widgets::shadow_spec("vocabulary.widget.saved.drop-shadow", SAVED),
    widgets::size_spec(
        "vocabulary.widget.saved.size",
        SAVED,
        TextSize::Medium,
        true,
    ),
    widgets::align_spec("vocabulary.widget.saved.align", SAVED, Align::Left),
    widgets::part_spec(
        "vocabulary.widget.saved.caption",
        SAVED,
        "vocabulary.part.caption",
        true,
    ),
    widgets::part_spec(
        "vocabulary.widget.saved.newest",
        SAVED,
        "vocabulary.part.newest",
        true,
    ),
    widgets::part_spec(
        "vocabulary.widget.saved.books",
        SAVED,
        "vocabulary.part.books",
        false,
    ),
    widgets::part_spec(
        "vocabulary.widget.saved.languages",
        SAVED,
        "vocabulary.part.languages",
        false,
    ),
];

/// The cards due's settings, kept in the program.
static DUE_SETTINGS: [SettingSpec; 10] = [
    widgets::frame_spec("vocabulary.widget.due.frame", DUE, Frame::None),
    widgets::padding_spec("vocabulary.widget.due.padding", DUE, Padding::Auto),
    widgets::paper_spec("vocabulary.widget.due.paper", DUE),
    widgets::ink_spec("vocabulary.widget.due.ink", DUE),
    widgets::corners_spec("vocabulary.widget.due.corners", DUE),
    widgets::frame_line_spec("vocabulary.widget.due.frame-line", DUE),
    widgets::shadow_spec("vocabulary.widget.due.drop-shadow", DUE),
    widgets::size_spec("vocabulary.widget.due.size", DUE, TextSize::Medium, true),
    widgets::align_spec("vocabulary.widget.due.align", DUE, Align::Left),
    widgets::part_spec(
        "vocabulary.widget.due.caption",
        DUE,
        "vocabulary.part.caption",
        true,
    ),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(WORD, "vocabulary.widget.word");
        &SPEC
    })?;
    r.add_settings(&WORD_SETTINGS)?;
    static WORD_WIDGET: [FixedWidget; 1] =
        [FixedWidget::new(WORD, "vocabulary.widget.word", |_| {
            Box::new(WordWidget::default())
        })
        .category(widgets::categories::WORDS)];
    r.add_widgets(&WORD_WIDGET)?;
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(SAVED, "vocabulary.widget.saved");
        &SPEC
    })?;
    r.add_settings(&SAVED_SETTINGS)?;
    static SAVED_WIDGET: [FixedWidget; 1] =
        [FixedWidget::new(SAVED, "vocabulary.widget.saved", |_| {
            Box::new(Saved::default())
        })
        .category(widgets::categories::WORDS)];
    r.add_widgets(&SAVED_WIDGET)?;
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(DUE, "vocabulary.widget.due");
        &SPEC
    })?;
    r.add_settings(&DUE_SETTINGS)?;
    static DUE_WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(DUE, "vocabulary.widget.due", |_| Box::new(Due::default()))
                .category(widgets::categories::WORDS),
        ];
    r.add_widgets(&DUE_WIDGET)
}

/// The height of a line of text `size` pixels large.
fn line(font: &FontHandle<'_>, size: f32) -> u16 {
    font.line_metrics(size).line_height as u16
}

/// The height `widget`'s frame takes (the SDK's).
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    paperpaw_sdk::widgets::frame_height(env, widget)
}

/// The caption's height, if `widget` shows it.
fn caption_room(env: &Env<'_>, widget: &str) -> u16 {
    if widgets::shows(env, widget, "caption") {
        widgets::caption_height(env)
    } else {
        0
    }
}

/// `widget`'s caption: "Word of the day" (or "Newest word"), "Vocabulary".
fn caption_text<'e>(env: &Env<'e>, widget: &str) -> &'e str {
    if widget == SAVED_NAME {
        env.i18n().tr("vocabulary.title")
    } else if widget == DUE_NAME {
        env.i18n().tr("vocabulary.cards.title")
    } else if env.settings().text(&format!("{WORD_NAME}.pick")) == PICKS[1] {
        env.i18n().tr("vocabulary.widget.newest")
    } else {
        env.i18n().tr("vocabulary.widget.word")
    }
}

/// The caption's height in `area`, if `widget` shows it, it fits the
/// width whole and there is room for a line of text below it as well
/// (else the content goes first, and the caption is left out rather
/// than cut).
fn caption_in(env: &Env<'_>, widget: &str, area: Rect) -> u16 {
    let style = UiStyle::new(env);
    let cap = caption_room(env, widget);
    let least = widgets::line_height(env, style.text());
    let fits = style.font(env).is_some_and(|font| {
        font.measure(style.small(), caption_text(env, widget)) <= f32::from(area.width)
    });
    if fits && cap + least <= area.height {
        cap
    } else {
        0
    }
}

/// Where content `height` high goes with `widget`'s caption above it
/// (if it shows), the two together in the middle of `area`: the block of
/// both, and the caption's height in it.
fn captioned_block(env: &Env<'_>, widget: &str, area: Rect, height: u16) -> (Rect, u16) {
    // Where they do not fit together, the content goes first.
    let cap = caption_in(env, widget, area);
    let cap = if cap + height <= area.height { cap } else { 0 };
    let total = (cap + height).min(area.height);
    let block = Rect::new(
        area.x,
        area.y + (area.height - total) / 2,
        area.width,
        total,
    );
    (block, cap)
}

/// Places content `height` high with `widget`'s caption `text` above
/// it (if it shows), the two together in the middle of `look`'s area,
/// and draws the caption; where the content goes.
fn captioned(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    look: &WidgetLook,
    text: &str,
    height: u16,
) -> Rect {
    let (block, cap) = captioned_block(env, widget, look.area, height);
    let total = block.height;
    if cap > 0 {
        let at = look.within(block);
        widgets::draw_caption(canvas, env, widget, &at, text);
    }
    Rect::new(block.x, block.y + cap, block.width, height.min(total - cap))
}

/// The size of `widget`'s main text: as its setting says ("as large as
/// fits": as large as its place holds), smaller where it is wider
/// than `width` (down to the body's size; then it is shortened) or a
/// line of it higher than `height` (down to the small size), measured
/// as drawn.
fn text_size(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    widget: &str,
    text: &str,
    (width, height): (u16, u16),
) -> f32 {
    let style = UiStyle::new(env);
    let max = match widgets::size(env, widget) {
        TextSize::Fit => f32::from(height).max(style.text()),
        size => size.text(&style),
    };
    let wide = f32::from(width);
    let tall = |size: f32| line(font, size) <= height;
    widgets::largest_size(max, style.text().min(max), |size| {
        tall(size) && font.measure(size, text) <= wide
    })
    .or_else(|| widgets::largest_size(max.min(style.text()), style.small(), tall))
    .unwrap_or(style.small().min(max))
}

/// What a widget says without words in `area` (its whole place): that
/// words looked up show here, or where that does not fit whole, that
/// there are none yet.
fn empty(env: &Env<'_>, widget: &str, area: Rect) -> Wrapped {
    let style = UiStyle::new(env);
    // A short message as wide as the spacing leaves, as high as half of
    // it: the lines' own room above and below the letters keeps them clear of the edges
    // (two lines fit a low place then).
    let inner = widgets::widget_look(env, widget, area).area;
    let (width, height) = (inner.width, (inner.height + area.height) / 2);
    // The sentence inside the spacing, the short form where it does not
    // fit whole.
    let (long, short) = if widget == DUE_NAME {
        (
            "vocabulary.widget.due.empty",
            "vocabulary.widget.due.empty.short",
        )
    } else {
        ("vocabulary.widget.empty", "vocabulary.widget.empty.short")
    };
    let long = env.i18n().tr(long);
    let wrapped = Wrapped::fitting(env, long, style.text(), style.small(), width, inner.height);
    if wrapped.whole() {
        return wrapped;
    }
    let short = env.i18n().tr(short);
    Wrapped::fitting(env, short, style.text(), style.small(), width, height)
}

/// Draws what a widget says without words, as laid out where it was
/// placed (made anew where it is drawn elsewhere).
fn draw_empty(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    (widget, look, area): (&str, &WidgetLook, Rect),
    laid: Option<&(Rect, Wrapped)>,
) {
    let laid = match laid {
        Some((at, laid)) if *at == area => Cow::Borrowed(laid),
        _ => Cow::Owned(empty(env, widget, area)),
    };
    let at = Rect::new(look.area.x, area.y, look.area.width, area.height);
    laid.draw(canvas, env, at, Align::Center, look.ink);
}

/// A tap opens the list of words.
fn open_words(area: Rect) -> Vec<WidgetTarget> {
    alloc::vec![WidgetTarget::new(area, Command::run(OPEN))]
}

/// The day it is where the reader is, counted from 1970; `None` where the
/// device does not know the time.
fn today(env: &Env<'_>) -> Option<i64> {
    let unix = paperpaw_sdk::time::unix_seconds(env)?;
    Some(paperpaw_sdk::time::local_seconds(env, unix).div_euclid(86_400))
}

/// The word to show: another one every day, or the newest. Where the
/// device does not know the day, one drawn at random each time the words
/// are read (at start, and when they change), else the newest.
fn pick(env: &Env<'_>) -> Option<Word> {
    if env.settings().text(&format!("{WORD_NAME}.pick")) == PICKS[1] {
        return store::summary(env.storage()).newest;
    }
    let count = store::count(env.storage());
    if count == 0 {
        return None;
    }
    let n = match today(env) {
        Some(day) => day.rem_euclid(count as i64) as usize,
        None => {
            let mut drawn = [0u8; 4];
            if env.random().fill(&mut drawn) {
                u32::from_le_bytes(drawn) as usize % count
            } else {
                count - 1
            }
        }
    };
    store::range(env.storage(), n, n + 1)
        .into_iter()
        .next()
        .map(|(_, word)| word)
}

/// The word of the day, read when updated.
#[derive(Default)]
struct WordWidget {
    word: Option<Word>,
    /// What it was read for: the day, the size of the file of words and
    /// the choice of word.
    read_for: Option<(Option<i64>, u64, bool)>,
    /// Where it is drawn, once placed, and the word laid out there.
    area: Option<Rect>,
    laid: Option<Laid>,
    /// What it says without words, laid out where it is placed.
    empty: Option<(Rect, Wrapped)>,
}

impl WordWidget {
    fn lay_out(&mut self, env: &Env<'_>) {
        self.laid = match (self.area, &self.word) {
            (Some(area), Some(word)) => lay_out(env, area, word),
            _ => None,
        };
        self.empty = match (self.area, &self.word) {
            (Some(area), None) => Some((area, empty(env, WORD_NAME, area))),
            _ => None,
        };
    }
}

/// The word of the day laid out in a widget's area, drawn as it is in
/// every band.
#[derive(Debug, Clone, PartialEq)]
struct Laid {
    /// The caption, the word, its sentence and its book together, and
    /// the caption's height in it (0 where it does not show).
    block: Rect,
    caption: u16,
    /// The word, as large as fits, and its line's height.
    size: f32,
    shown: String,
    big: u16,
    /// The sentence: the lines it needs, three at most, as far as they
    /// fit.
    sentence: Option<Wrapped>,
    sentence_h: u16,
    /// The book it is from, if shown.
    book: Option<String>,
}

/// `word` laid out in widget area `area`: the caption, the word, its
/// sentence (as many lines as fit, three at most) and its book.
fn lay_out(env: &Env<'_>, area: Rect, word: &Word) -> Option<Laid> {
    let style = UiStyle::new(env);
    let font = style.font(env)?;
    let id = WORD_NAME;
    let small = line(&font, style.small());
    let area = widgets::widget_look(env, id, area).area;
    let room = area.height.saturating_sub(caption_in(env, id, area));
    let width = f32::from(area.width);
    let book = if widgets::shows(env, id, "book") && !word.title.is_empty() {
        small
    } else {
        0
    };
    let size = text_size(
        env,
        &font,
        id,
        &word.word,
        (area.width, room.saturating_sub(book)),
    );
    let big = line(&font, size);
    // The sentence on as many lines as it needs and fit, three at most.
    let fit = (room.saturating_sub(big + book) / small.max(1)).min(3);
    let sentence = (widgets::shows(env, id, "sentence") && !word.sentence.is_empty() && fit > 0)
        .then(|| Wrapped::new(env, &word.sentence, style.small(), area.width, fit * small));
    let sentence_h = sentence.as_ref().map_or(0, |s| s.height(env));
    let (block, caption) = captioned_block(env, id, area, big + sentence_h + book);
    let from = (book > 0).then(|| {
        let text = env.i18n().format(
            "vocabulary.widget.from",
            &[("title", Arg::Str(&word.title))],
        );
        ui::fit(&font, style.small(), &text, width)
    });
    Some(Laid {
        block,
        caption,
        size,
        shown: ui::fit(&font, size, &word.word, width),
        big,
        sentence,
        sentence_h,
        book: from,
    })
}

impl Widget for WordWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        // Read again only when the day, the words or the choice changed:
        // it is asked each minute, for the day.
        let size = env.storage().size(&store::file()).unwrap_or(0);
        let newest = env.settings().text(&format!("{WORD_NAME}.pick")) == PICKS[1];
        let read_for = Some((today(env), size, newest));
        if read_for == self.read_for {
            return false;
        }
        self.read_for = read_for;
        let word = pick(env);
        let changed = word != self.word;
        self.word = word;
        if changed {
            self.lay_out(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.lay_out(env);
        }
    }

    fn live(&self) -> bool {
        // Another word at midnight.
        true
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = WORD_NAME;
        let size = widgets::size(env, id).text(&style);
        let small = line(&font, style.small());
        let sentence = if widgets::shows(env, id, "sentence") {
            small * 2
        } else {
            0
        };
        let book = if widgets::shows(env, id, "book") {
            small
        } else {
            0
        };
        frame_height(env, id) + caption_room(env, id) + line(&font, size) + sentence + book
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WORD_NAME, area);
        let Some(word) = &self.word else {
            let at = (WORD_NAME, &look, area);
            draw_empty(canvas, env, at, self.empty.as_ref());
            return;
        };
        // Laid out when placed; drawn elsewhere, laid out for that.
        let laid = match &self.laid {
            Some(laid) if self.area == Some(area) => Cow::Borrowed(laid),
            _ => match lay_out(env, area, word) {
                Some(laid) => Cow::Owned(laid),
                None => return,
            },
        };
        draw_word(canvas, env, &look, &laid);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        open_words(area)
    }
}

/// The caption, the word, its sentence and its book, as laid out.
fn draw_word(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, laid: &Laid) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let mut ui = ui::Ui::new(canvas, env, &style);
    let id = WORD_NAME;
    let area = look.area;
    if laid.caption > 0 {
        let caption = caption_text(env, id);
        let at = look.within(laid.block);
        widgets::draw_caption(ui.canvas(), env, id, &at, caption);
    }
    let align = widgets::align(env, id);
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    let top = laid.block.y + laid.caption;
    let x = align.x(area, font.measure(laid.size, &laid.shown));
    let shown = &laid.shown;
    ui::Label::new(shown)
        .px(laid.size)
        .color(look.ink)
        .draw_from(&mut ui, x, row(top, laid.big));
    let y = top + laid.big;
    if let Some(sentence) = &laid.sentence {
        sentence.draw(ui.canvas(), env, row(y, laid.sentence_h), align, look.faint);
    }
    if let Some(book) = &laid.book {
        let small = line(&font, style.small());
        let x = align.x(area, font.measure(style.small(), book));
        let at = row(y + laid.sentence_h, small);
        ui::Label::new(book)
            .px(style.small())
            .color(look.faint)
            .draw_from(&mut ui, x, at);
    }
}

/// The words saved, read when updated.
#[derive(Default)]
struct Saved {
    summary: Summary,
    /// Where it is drawn, once placed, and what it says there without
    /// words.
    area: Option<Rect>,
    empty: Option<(Rect, Wrapped)>,
}

impl Saved {
    fn lay_out(&mut self, env: &Env<'_>) {
        self.empty = match self.area {
            Some(area) if self.summary.count == 0 => Some((area, empty(env, SAVED_NAME, area))),
            _ => None,
        };
    }
}

impl Widget for Saved {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let summary = store::summary(env.storage());
        let changed = summary != self.summary;
        self.summary = summary;
        if changed {
            self.lay_out(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.lay_out(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = SAVED_NAME;
        let size = widgets::size(env, id).text(&style);
        let notes = notes(env, &self.summary, |_| true).len() as u16;
        frame_height(env, id)
            + caption_room(env, id)
            + line(&font, size)
            + notes * line(&font, style.small())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, SAVED_NAME, area);
        if self.summary.count == 0 {
            let at = (SAVED_NAME, &look, area);
            draw_empty(canvas, env, at, self.empty.as_ref());
            return;
        }
        draw_saved(canvas, env, &look, &self.summary);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        open_words(area)
    }
}

/// The notes below the count: the newest word, where the words come
/// from (on one line where it `fits`, else a line each).
fn notes(env: &Env<'_>, summary: &Summary, fits: impl Fn(&str) -> bool) -> Vec<String> {
    let id = SAVED_NAME;
    let mut notes = Vec::new();
    if let Some(newest) = summary
        .newest
        .as_ref()
        .filter(|_| widgets::shows(env, id, "newest"))
    {
        notes.push(env.i18n().format(
            "vocabulary.widget.latest",
            &[("word", Arg::Str(&newest.word))],
        ));
    }
    let mut from: Vec<String> = Vec::new();
    if widgets::shows(env, id, "books") && summary.books > 0 {
        from.push(
            env.i18n()
                .plural("vocabulary.widget.books", summary.books as i64, &[]),
        );
    }
    if widgets::shows(env, id, "languages") && summary.languages > 0 {
        from.push(
            env.i18n()
                .plural("vocabulary.widget.languages", summary.languages as i64, &[]),
        );
    }
    let joined = from.join(" · ");
    if fits(&joined) {
        notes.push(joined);
    } else {
        notes.extend(from);
    }
    notes
}

/// "128 words", and the notes below.
fn draw_saved(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, summary: &Summary) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = SAVED_NAME;
    let small = line(&font, style.small());
    let area = look.area;
    let room = area.height.saturating_sub(caption_in(env, id, area));
    let headline = env
        .i18n()
        .plural("vocabulary.widget.words", summary.count as i64, &[]);
    let width = f32::from(area.width);
    let fits = |text: &str| text.is_empty() || font.measure(style.small(), text) <= width;
    let mut notes = notes(env, summary, fits);
    notes.retain(|n| !n.is_empty() && fits(n));
    let least = line(&font, style.text());
    while !notes.is_empty() && least + notes.len() as u16 * small > room {
        notes.pop();
    }
    let notes_h = notes.len() as u16 * small;
    let size = text_size(env, &font, id, &headline, (area.width, room - notes_h));
    let big = line(&font, size);
    let caption = caption_text(env, id);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let block = captioned(ui.canvas(), env, id, look, caption, big + notes_h);
    let align = widgets::align(env, id);
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    // Too wide: the number alone ("128 words": "128").
    let headline = if font.measure(size, &headline) > width {
        format!("{}", summary.count)
    } else {
        headline
    };
    let headline = ui::fit(&font, size, &headline, width);
    let x = align.x(area, font.measure(size, &headline));
    ui::Label::new(&headline)
        .px(size)
        .color(look.ink)
        .draw_from(&mut ui, x, row(block.y, big));
    for (n, note) in notes.iter().enumerate() {
        let note = ui::fit(&font, style.small(), note, width);
        let x = align.x(area, font.measure(style.small(), &note));
        let at = row(block.y + big + n as u16 * small, small);
        ui::Label::new(&note)
            .px(style.small())
            .color(look.faint)
            .draw_from(&mut ui, x, at);
    }
}

/// How many flash cards are due; without decks, a word on how to make
/// some.
#[derive(Default)]
struct Due {
    /// `None` without decks.
    count: Option<usize>,
    area: Option<Rect>,
    empty: Option<(Rect, Wrapped)>,
}

impl Due {
    fn lay_out(&mut self, env: &Env<'_>) {
        self.empty = match self.area {
            Some(area) if self.count.is_none() => Some((area, empty(env, DUE_NAME, area))),
            _ => None,
        };
    }
}

impl Widget for Due {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let count = (!crate::cards::decks(env.storage())
            .unwrap_or_default()
            .is_empty())
        .then(|| crate::decks::due_now(env));
        let changed = count != self.count;
        self.count = count;
        if changed {
            self.lay_out(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.lay_out(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let size = widgets::size(env, DUE_NAME).text(&style);
        frame_height(env, DUE_NAME) + caption_room(env, DUE_NAME) + line(&font, size)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, DUE_NAME, area);
        let Some(count) = self.count else {
            let at = (DUE_NAME, &look, area);
            draw_empty(canvas, env, at, self.empty.as_ref());
            return;
        };
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return;
        };
        let inner = look.area;
        let room = inner
            .height
            .saturating_sub(caption_in(env, DUE_NAME, inner));
        let headline = env
            .i18n()
            .plural("vocabulary.widget.due.count", count as i64, &[]);
        let width = f32::from(inner.width);
        // Too wide: the number alone.
        let size = text_size(env, &font, DUE_NAME, &headline, (inner.width, room));
        let headline = if font.measure(size, &headline) > width {
            format!("{count}")
        } else {
            headline
        };
        let big = line(&font, size);
        let mut ui = ui::Ui::new(canvas, env, &style);
        let block = captioned(
            ui.canvas(),
            env,
            DUE_NAME,
            &look,
            caption_text(env, DUE_NAME),
            big,
        );
        let headline = ui::fit(&font, size, &headline, width);
        let x = widgets::align(env, DUE_NAME).x(inner, font.measure(size, &headline));
        let at = Rect::new(inner.x, block.y, inner.width, big);
        ui::Label::new(&headline)
            .px(size)
            .color(look.ink)
            .draw_from(&mut ui, x, at);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget::new(area, Command::run(crate::CARDS))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings kept in the program are named after their widget, as
    /// it reads them.
    #[test]
    fn settings_are_the_widgets() {
        let all: [(&[SettingSpec], &str); 3] = [
            (&WORD_SETTINGS, WORD_NAME),
            (&SAVED_SETTINGS, SAVED_NAME),
            (&DUE_SETTINGS, DUE_NAME),
        ];
        for (settings, widget) in all {
            for spec in settings {
                let prefix = format!("{widget}.");
                assert!(spec.id.as_str().starts_with(&prefix), "{}", spec.id);
                assert_eq!(spec.group.as_str(), widget);
            }
        }
    }
}
