//! The reader's screen: a book shown page by page, and what the reader
//! does on it.

mod book;
mod draw;
mod hands_free;
mod linking;
mod marks;
mod sample;
mod selecting;
mod typography;
mod words;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, fonts, params, slots};
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::{Document, DocumentError, Link, Locator};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::layout::{Hyphenate, LayoutContext, Page, PageLine, layout_page, line_start};
use paperpaw_sdk::status::context as status_context;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, FontHandle, Gesture, GestureKind, Handled, Id, Params,
    Point, Rect, RefreshMode, Screen,
};

use crate::annotations::{self, Annotation, Kind, Mark};
use crate::selection::{self, Item, Mode, Selection};
use crate::settings::{
    FONT_SIZE, FONT_WEIGHT, HYPHENATE, INDENT, JUSTIFY, LINE_SPACING, MARGIN, MARGIN_ENDS,
    PARAGRAPH_SPACING, WORD_SPACING,
};
use crate::status;
use crate::{PLUGIN, state_path};
use paperpaw_sdk::reading::{FilteredDocument, PageIndex};

use self::book::*;
pub(crate) use self::book::{book_dir, remember_last};
use self::hands_free::*;
use self::linking::*;
pub(crate) use self::sample::Sample;
use self::typography::*;
pub(crate) use self::words::WordBox;
use self::words::*;
pub(crate) use paperpaw_sdk::reading::open_options;

/// The kernel's settings of the type of book pages: a change lays the
/// book out anew (the font and the book's own styles open it again).
const TYPE_SETTINGS: [&str; 14] = [
    FONT_SIZE.static_str(),
    FONT_WEIGHT.static_str(),
    WORD_SPACING.static_str(),
    LINE_SPACING.static_str(),
    MARGIN.static_str(),
    MARGIN_ENDS.static_str(),
    JUSTIFY.static_str(),
    HYPHENATE.static_str(),
    paperpaw_sdk::builtin::settings::HYPHEN_MIN_LEFT.static_str(),
    paperpaw_sdk::builtin::settings::HYPHEN_MIN_RIGHT.static_str(),
    INDENT.static_str(),
    PARAGRAPH_SPACING.static_str(),
    paperpaw_sdk::builtin::settings::WIDOWS.static_str(),
    paperpaw_sdk::builtin::settings::ORPHANS.static_str(),
];

/// Pages laid out per background step; small enough to keep input snappy.
const PAGES_PER_STEP: usize = 8;

/// On small devices, where laying out a page takes half a second (the X4),
/// one: buttons wait no longer than that.
const PAGES_PER_STEP_SMALL: usize = 1;

/// Pause between background steps.
const STEP_DELAY_MS: u64 = 2;
/// After a press or a tap, how long the status lines wait to be refreshed
/// on their own: a page turned meanwhile draws them with it, in one
/// refresh, and a refresh of their own would hold up the next press.
const STATUS_QUIET_MS: u64 = 2_000;

/// On small devices a rest as long as a step: the CPU rests (and the X4
/// sleeps lightly) half the time while pages are counted, instead of
/// running flat out for minutes.
const STEP_DELAY_SMALL_MS: u64 = 500;

/// The position is written this long after the last page turned, never
/// while a page turns: a card is slow to write to, and a page turned
/// should not wait for it. Always when the book is closed or the device
/// sleeps or restarts.
const SAVE_QUIET_MS: u64 = 2_000;

/// While pages turn on without a rest, the position is written at least
/// this often all the same.
const SAVE_EVERY_MS: u64 = 5000;

/// Pages remembered for going back before the index covers them.
const HISTORY: usize = 64;

/// Page indexes of a book's layouts kept before a new one removes them.
const INDEXES_KEPT: usize = 4;

/// The least text, in bytes, a page back goes where the page index does
/// not reach (the page shown holding less, e.g. a picture).
const MIN_PAGE_BACK: u32 = 200;

/// Pages laid out at most to find one that reaches the page left, where
/// the page index does not reach (each takes half a second on the X4).
const PAGE_BACK_TRIES: usize = 3;

/// Places jumps were made from (links, contents, search, page numbers),
/// kept for *Back*.
const RETURNS: usize = 32;

/// Shows a book page by page.
pub(crate) struct ReaderScreen {
    book: Result<Book, String>,
    /// Text chosen on the page (see `selection`), and the page's start.
    selection: Option<Selection>,
    selected_on: Locator,
    /// A link being chosen with the buttons.
    linking: Option<Linking>,
    /// The last search, offered again.
    last_search: String,
    /// What the status lines showed when last drawn.
    status_shown: Option<u64>,
    /// When the last press or tap came.
    input_at: Option<paperpaw_sdk::Instant>,
    /// The sentence being read aloud.
    aloud: Option<Aloud>,
    /// Where reading aloud was paused: the page and its word.
    resume: Option<(Locator, usize)>,
    /// When the page turns by itself next, while it does.
    auto_turn: Option<paperpaw_sdk::Instant>,
    /// Where skimming began, while skimming.
    skim_from: Option<Locator>,
    /// The book to open once the screen says it is being prepared (the
    /// first opening takes a while).
    preparing: Option<Params>,
    /// The book not there yet (`DocumentError::NotReady`), opened when
    /// its format says it is (`events::DOCUMENT_READY`).
    waiting: Option<Params>,
    /// Pages with smooth letters (in grays, where the panel can).
    smooth: bool,
    /// The book let go of while a screen above needed the memory, its
    /// place kept: opened again as it shows (see `on_squeeze`).
    squeezed: Option<Params>,
}

impl ReaderScreen {
    pub(crate) fn open(env: &Env<'_>, params: &Params) -> Self {
        let prepared = params.get(params::PATH).is_none_or(|path| {
            env.formats()
                .prepared(env.storage(), path, &open_options(env))
        });
        let (book, preparing, waiting) = if prepared {
            let (book, not_ready) = open_book(env, params);
            (book, None, not_ready.then(|| params.clone()))
        } else {
            let message = env.i18n().tr("reader.preparing").to_string();
            (Err(message), Some(params.clone()), None)
        };
        Self {
            smooth: env.settings().read(&crate::settings::SMOOTH_TEXT),
            book,
            preparing,
            waiting,
            selection: None,
            selected_on: Locator::START,
            linking: None,
            last_search: String::new(),
            status_shown: None,
            input_at: None,
            aloud: None,
            resume: None,
            auto_turn: None,
            skim_from: None,
            squeezed: None,
        }
    }

    /// Opens the book again where it was let go of for memory.
    fn unsqueeze(&mut self, env: &Env<'_>) {
        if let Some(params) = self.squeezed.take() {
            let (book, not_ready) = open_book(env, &params);
            self.book = book;
            self.waiting = not_ready.then_some(params);
        }
    }

    /// Opens the book the screen says is being prepared, if it does;
    /// whether it did.
    fn prepare(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let Some(params) = self.preparing.take() else {
            return false;
        };
        let env = *cx.env();
        let (book, not_ready) = open_book(&env, &params);
        self.book = book;
        self.waiting = not_ready.then_some(params);
        // Shown as a screen opening is: the book replaces the notice.
        cx.request_redraw();
        let flash = env
            .settings()
            .bool(paperpaw_sdk::builtin::settings::FLASH_SCREENS.as_str());
        cx.request_refresh(if flash {
            RefreshMode::Full
        } else {
            RefreshMode::Partial
        });
        self.on_show(cx);
        true
    }

    /// Opens the skimming overlay where the book is.
    fn open_skim(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        self.skim_from.get_or_insert(book.page.start);
        let skimmed = book.skimmed(&env).marks(book.chapter_marks.clone());
        cx.command(Command::OpenSlot(slots::SKIM, skimmed.to_params()));
    }

    /// Moves as the skimming overlay asks, and tells it where the book is.
    fn skim(&mut self, to: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let from = self.skim_from;
        let Ok(book) = &mut self.book else {
            return;
        };
        let target = if to == "back" {
            from
        } else {
            book.skim_target(&env, to)
        };
        if let Some(target) = target {
            let start = book.page_start(&env, target);
            book.show(cx, start, true);
        }
        cx.command(events::SKIMMED.emit(book.skimmed(&env)));
    }

    /// Shows the next status line at the top (or bottom) and keeps the
    /// choice.
    fn next_status_line(&mut self, top: bool, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &mut self.book else {
            return;
        };
        let slot = usize::from(!top);
        let next = status::next(&env, top, &book.status_lines[slot]).to_string();
        if next == status::current(&env, top, &book.status_lines[slot]) {
            return;
        }
        book.status_lines[slot] = next;
        status::save_choice(env.storage(), &book.status_lines);
        for (area, at_top) in book.status_areas() {
            if at_top == top {
                cx.request_redraw_area(area);
            }
        }
        cx.request_refresh(RefreshMode::Fast);
        self.status_shown = None;
    }

    /// Redraws the status lines if what they show changed, and checks again
    /// when the minute changes.
    fn update_status(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        if book.status_areas().next().is_none() {
            return;
        }
        let signature = status::signature(&env, &book.status_context(&env), &book.status_lines);
        if Some(signature) != self.status_shown
            && let Some(quiet) = self
                .input_at
                .map(|at| at.add_millis(STATUS_QUIET_MS))
                .filter(|quiet| *quiet > env.now())
        {
            // Pages turned a moment ago: later, or with the next page.
            cx.wake_at(quiet);
        } else if Some(signature) != self.status_shown {
            self.status_shown = Some(signature);
            for (area, _) in book.status_areas() {
                cx.request_redraw_area(area);
            }
            cx.request_refresh(RefreshMode::Fast);
        }
        let next = paperpaw_sdk::time::next_minute(&env).unwrap_or(env.now().add_millis(60_000));
        cx.wake_at(next);
        if let Some(at) = self.auto_turn {
            cx.wake_at(at);
        }
    }

    /// Asks on the keyboard what to search for, or which page to go to.
    fn ask(&mut self, action: &Id, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &self.book else {
            return;
        };
        let question = if *action == crate::actions::SEARCH {
            ask::text(
                env.i18n().tr("reader.search.prompt"),
                &crate::SEARCH_ENTERED,
            )
            .initial(self.last_search.clone())
        } else if *action == crate::actions::GO_TO_PERCENT {
            ask::text(
                env.i18n().tr("reader.go-to-percent.prompt"),
                &crate::PERCENT_ENTERED,
            )
            .number()
        } else {
            let (pages, complete) = book
                .index
                .as_ref()
                .map_or((0, false), |i| (i.pages(), i.is_complete()));
            let key = if complete {
                "reader.go-to-page.prompt"
            } else {
                "reader.go-to-page.counting"
            };
            let title = env
                .i18n()
                .format(key, &[("pages", Arg::Int(i64::from(pages)))]);
            ask::text(title, &crate::PAGE_ENTERED).number()
        };
        cx.command(question.with(params::PATH, book.path.clone()).command());
    }

    /// Sets or removes the bookmark of the page.
    fn toggle_bookmark(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &mut self.book else {
            return;
        };
        let (start, end) = (book.page.start, book.page.end);
        let on_page = |m: &Mark| m.kind == Kind::Bookmark && m.start >= start && m.start < end;
        if !matches!(
            annotations::remove(env.storage(), &book.path, on_page),
            Ok(true)
        ) {
            let text = page_text(&book.page.lines);
            let kept = annotations::add(
                env.storage(),
                &book.path,
                Annotation::new(
                    Mark::new(Kind::Bookmark, start, start, false),
                    (book.percent() + 0.5) as u8,
                    paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0),
                    0,
                    annotations::snippet(&text),
                    String::new(),
                ),
            );
            if kept.is_err() {
                marks_not_saved(cx);
            }
        }
        book.reload_marks(&env);
        cx.command(annotations::changed(&book.path));
        cx.request_redraw_area(book.ribbon_area(&env));
        cx.request_refresh(RefreshMode::Fast);
    }
}

impl Screen for ReaderScreen {
    /// A page with pictures, or any page with smooth letters: in grays
    /// where the panel can. Without smooth letters a page of text alone
    /// is black and white and turns without a refresh in grays.
    fn grays(&self) -> bool {
        self.book
            .as_ref()
            .is_ok_and(|book| self.smooth || book.pictured())
    }

    /// It says itself which pages are best in grays ([`Screen::grays`]):
    /// pages of text turn in black and white.
    fn pictures_in_grays(&self) -> bool {
        false
    }

    /// *Select* opens the reading menu, so its hint says so.
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM {
            paperpaw_sdk::Hint::Text(env.i18n().tr("core.hint.core.menu").to_string())
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    /// The book is closed: said, where it was left.
    /// A screen above needs the memory: the book is let go of, its place
    /// kept, and opened there again as it shows (not a page a plugin
    /// shows, nor while it reads aloud or turns its pages).
    fn on_squeeze(&mut self, _: &paperpaw_sdk::realtime::Squeeze, cx: &mut Cx<'_, '_>) -> Handled {
        let Ok(book) = &mut self.book else {
            return Handled::No;
        };
        if book.web() || book.page_back || self.aloud.is_some() || self.auto_turn.is_some() {
            return Handled::No;
        }
        book.save_due(cx, true);
        self.squeezed = Some(Params::new().with(params::PATH, book.path.clone()));
        self.book = Err(String::new());
        self.selection = None;
        self.linking = None;
        Handled::Yes
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.unsqueeze(&env);
        if let Ok(book) = &mut self.book {
            book.save_due(cx, true);
            // With its XPointer, for a sync (found when the place was
            // kept, else now).
            if book.xpointer.is_none() {
                book.xpointer = book.document.xpointer(env.storage(), book.page.start);
            }
        }
        if let Ok(book) = &self.book
            && !book.web()
        {
            let reading = book.reading(&env).xpointer(book.xpointer.clone());
            cx.command(events::BOOK_CLOSED.emit(reading));
        }
    }

    /// A book whose pages go right to left: taps and swipes the other way.
    fn mirrored(&self) -> bool {
        self.book.as_ref().is_ok_and(|book| {
            book.document.metadata().direction
                == paperpaw_sdk::document::PageProgression::RightToLeft
        })
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.unsqueeze(env);
        self.draw_book(canvas, env);
        // Kept from the first band of a frame to its last only: on the X4
        // a page's words take some 15 KiB.
        if let Ok(book) = &mut self.book
            && canvas.clip().bottom() >= u32::from(env.device().display.height)
        {
            book.words.take();
        }
    }
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.unsqueeze(cx.env());
        if self.preparing.is_some() {
            // Shown saying so first, then prepared.
            let now = cx.env().now();
            cx.wake_at(now);
            return;
        }
        // Done skimming: *Back* returns to where it began.
        if let (Some(from), Ok(book)) = (self.skim_from.take(), &mut self.book)
            && book.page.start != from
        {
            if book.returns.len() == RETURNS {
                book.returns.remove(0);
            }
            book.returns.push(from);
            book.landed = Some(book.page.start);
        }
        // *Back* is drawn anew: screens over it may have hidden it.
        if let Ok(book) = &self.book
            && let Some(area) = book.back_area(cx.env())
        {
            cx.request_redraw_area(area);
        }
        if let Ok(book) = &self.book {
            // The book's own settings ("Only this book") while it is open.
            cx.command(Command::SetScope(Some(format!(
                "{}/settings.ini",
                book.dir
            ))));
            if !book.web() {
                cx.command(events::BOOK_OPENED.emit(book.reading(cx.env())));
            }
            // The book shows: its hints may, the first time.
            cx.command(paperpaw_sdk::help::ready(&crate::SCREEN));
            let now = cx.env().now();
            cx.wake_at(now);
        }
        // Time passed while another screen was open.
        self.update_status(cx);
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.prepare(cx) {
            return;
        }
        let Ok(book) = &mut self.book else {
            return;
        };
        book.save_due(cx, false);
        if book.index_step(&env) {
            let delay = match env.device().memory {
                paperpaw_sdk::MemoryClass::Small => STEP_DELAY_SMALL_MS,
                _ => STEP_DELAY_MS,
            };
            cx.wake_at(env.now().add_millis(delay));
        }
        self.auto_turn_due(cx);
        self.let_go_of_other_pages(cx);
        // Page numbers once counted, the time, the battery, …
        self.update_status(cx);
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        self.input_at = Some(cx.env().now());
        // A button pressed while the book is being prepared waits for it.
        self.prepare(cx);
        // Any button pauses reading aloud (also "Read aloud"), and does
        // nothing else.
        if self.aloud.is_some() {
            self.pause_aloud(cx);
            return Handled::Yes;
        }
        // The arrows turn pages as the page buttons do: the X4's buttons
        // in front, a keyboard's arrows; not while text or a link is
        // chosen, where they move by words and lines.
        let choosing = self.selection.is_some() || self.linking.is_some();
        let turn = page_turn(action).filter(|_| !choosing);
        let action = turn.as_ref().unwrap_or(action);
        if *action == crate::actions::READ_ALOUD {
            self.start_aloud(cx);
            return Handled::Yes;
        }
        if *action == crate::actions::AUTO_TURN {
            self.toggle_auto_turn(cx);
            return Handled::Yes;
        }
        if *action == crate::actions::SKIM_BOOK {
            self.open_skim(cx);
            return Handled::Yes;
        }
        // While pages turn by themselves, turning one starts the count
        // anew; the menu (a tap in the middle) stops it instead.
        if self.auto_turn.is_some() {
            let env = *cx.env();
            if *action == actions::MENU || *action == actions::CONFIRM {
                self.toggle_auto_turn(cx);
                return Handled::Yes;
            }
            if *action == actions::NEXT_PAGE || *action == actions::PREV_PAGE {
                let seconds = self.auto_turn_seconds(&env);
                self.auto_turn = Some(env.now().add_millis(seconds * 1000));
                if let Some(at) = self.auto_turn {
                    cx.wake_at(at);
                }
            }
        }
        if let Some(selection) = self.selection {
            return self.on_selection(action, selection, cx);
        }
        if let Some(linking) = self.linking {
            return self.on_linking(action, linking, cx);
        }
        if *action == crate::actions::LINKS {
            let env = *cx.env();
            if let Ok(book) = &self.book {
                let focus = (!book.targets(&env).is_empty()).then_some(0);
                self.set_linking(Some(Linking { focus }), cx);
            }
            return Handled::Yes;
        }
        if *action == crate::actions::SELECT {
            // Choose text with the buttons, starting at the first word.
            let env = *cx.env();
            if let Ok(book) = &self.book {
                let words = book.words(&env);
                let first = words.iter().position(|w| !w.lookup_text().is_empty());
                self.set_selection(first.map(Selection::choosing), cx);
            }
            return Handled::Yes;
        }
        if *action == crate::actions::TOGGLE_BOOKMARK {
            self.toggle_bookmark(cx);
            return Handled::Yes;
        }
        if *action == crate::actions::NEXT_STATUS_LINE
            || *action == crate::actions::NEXT_TOP_STATUS_LINE
        {
            let env = *cx.env();
            let top =
                *action == crate::actions::NEXT_TOP_STATUS_LINE || !status::shown(&env, false);
            self.next_status_line(top, cx);
            return Handled::Yes;
        }
        if *action == crate::actions::SEARCH
            || *action == crate::actions::GO_TO_PAGE
            || *action == crate::actions::GO_TO_PERCENT
        {
            self.ask(action, cx);
            return Handled::Yes;
        }
        let Ok(book) = &mut self.book else {
            return Handled::No;
        };
        if *action == actions::NEXT_PAGE {
            let end = book.page.end;
            book.show(cx, end, true);
            Handled::Yes
        } else if *action == actions::PREV_PAGE {
            let env = *cx.env();
            if let Some(start) = book.previous_start(&env) {
                book.show(cx, start, false);
            }
            Handled::Yes
        } else if *action == crate::actions::NEXT_CHAPTER || *action == crate::actions::PREV_CHAPTER
        {
            let env = *cx.env();
            let to = match *action == crate::actions::NEXT_CHAPTER {
                true => "chapter:+1",
                false => "chapter:-1",
            };
            if let Some(target) = book.skim_target(&env, to) {
                let start = book.page_start(&env, target);
                book.show(cx, start, false);
            }
            Handled::Yes
        } else if *action == crate::actions::BOOK_INFO {
            cx.command(Command::OpenSlot(
                slots::BOOK_INFO,
                Params::new().with(params::PATH, book.path.clone()),
            ));
            Handled::Yes
        } else if *action == crate::actions::TOC {
            cx.command(Command::OpenSlot(
                slots::TOC,
                crate::toc::locator_params(&book.path, book.page.start),
            ));
            Handled::Yes
        } else if *action == crate::actions::ANNOTATIONS {
            cx.command(Command::OpenSlot(
                slots::ANNOTATIONS,
                Params::new().with(params::PATH, book.path.clone()),
            ));
            Handled::Yes
        } else if *action == actions::BACK && !book.returns.is_empty() {
            book.go_back(cx);
            Handled::Yes
        } else if *action == actions::MENU || *action == actions::CONFIRM {
            // As a bar at the top, with the book and where one is in it.
            let env = *cx.env();
            let chapter = book
                .chapter
                .as_ref()
                .map(|(_, _, title)| title.clone())
                .unwrap_or_default();
            let reading = book.reading(&env);
            let page = match (reading.page, reading.pages) {
                (Some(page), Some(pages)) => format!("{page} / {pages}"),
                _ => format!("{} %", reading.progress),
            };
            // Pictures of pages: no text to search, hear or set.
            let mut hide: Vec<Id> = Vec::new();
            if book.document.whole_pages() {
                hide.extend([
                    crate::actions::SEARCH,
                    crate::actions::READ_ALOUD,
                    crate::actions::QUICK_SETTINGS,
                ]);
                if book.document.toc_len(env.storage()) == 0 {
                    hide.push(crate::actions::TOC);
                }
            }
            let hide: Vec<&str> = hide.iter().map(Id::as_str).collect();
            cx.command(Command::OpenSlot(
                slots::MENU,
                Params::new()
                    .with(params::MENU, crate::MENU.as_str())
                    .with(params::HIDE, hide.join(","))
                    .with(params::MODE, "bar")
                    .with(params::TITLE, book.title.clone())
                    .with(params::TEXT, chapter)
                    .with(params::PAGE, page)
                    .with(params::ACTION, crate::actions::BOOK_INFO.as_str()),
            ));
            Handled::Yes
        } else {
            Handled::No
        }
    }

    /// A tap on a link follows it, a long press selects the word under
    /// the finger; while text is selected, taps extend the selection or
    /// choose an item of the bar.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        self.input_at = Some(cx.env().now());
        // As does any tap.
        if self.aloud.is_some() {
            self.pause_aloud(cx);
            return Handled::Yes;
        }
        if self.linking.is_some() {
            self.set_linking(None, cx);
            return Handled::Yes;
        }
        let env = *cx.env();
        // A tap on a status line shows the next one (not where the
        // bookmark corner or the way back are).
        if let (GestureKind::Tap, None, Ok(book)) = (gesture.kind, self.selection, &self.book) {
            let p = gesture.start;
            let corner = env.settings().read(&crate::settings::CORNER_BOOKMARK)
                && book.corner_area(&env).contains(p);
            let back = book.back_area(&env).is_some_and(|a| a.contains(p));
            if !corner
                && !back
                && let Some(top) = book.status_hit(&env, p)
            {
                // An item that does something when tapped, else the next
                // line.
                match book.status_tap(&env, top, p) {
                    Some(command) => cx.command(command),
                    None => self.next_status_line(top, cx),
                }
                return Handled::Yes;
            }
            // An item a layer drew: its plugin answers, before anything
            // else the reader would do with the tap there.
            if !corner
                && !back
                && let Some((layer, item)) = book
                    .layer_items(&env)
                    .into_iter()
                    .find(|(_, item)| item.area.contains(p))
            {
                book.tap_item(&layer, &item, Some(p), cx);
                return Handled::Yes;
            }
            // The room of a layer that hears taps: its plugin answers (not
            // where the bookmark corner or the way back are).
            if !corner
                && !back
                && let Some(layer) =
                    env.page_layers()
                        .tapped(&env, &book.typography.rooms, p.x, p.y)
            {
                book.tap_layer(layer, Some(p), cx);
                return Handled::Yes;
            }
        }
        let Ok(book) = &self.book else {
            return Handled::No;
        };
        let words = book.words(&env);
        let at = |point: Point| {
            words.iter().position(|word| {
                let area = word.area;
                // A little slack between words and lines.
                let slack = 4;
                point.x >= i32::from(area.x) - slack
                    && point.x < area.right() as i32 + slack
                    && point.y >= i32::from(area.y)
                    && point.y < area.bottom() as i32
            })
        };
        let link = at(gesture.start)
            .and_then(|i| book.link_of(&words[i]))
            .map(|link| (link.href.clone(), link_label(book, words, link)));
        // A mark whose plugin hears taps on it.
        let tapped = at(gesture.start).and_then(|i| book.tappable_at(words, i));
        match (gesture.kind, self.selection) {
            (GestureKind::Tap, None)
                if env.settings().read(&crate::settings::CORNER_BOOKMARK)
                    && book.corner_area(&env).contains(gesture.start) =>
            {
                self.toggle_bookmark(cx);
                Handled::Yes
            }
            (GestureKind::Tap, None)
                if book
                    .back_area(&env)
                    .is_some_and(|area| area.contains(gesture.start)) =>
            {
                if let Ok(book) = &mut self.book {
                    if book.returns.is_empty() {
                        // Out of the web page, to the one before.
                        cx.command(Command::Back);
                    } else {
                        book.go_back(cx);
                    }
                }
                Handled::Yes
            }
            (GestureKind::Tap, None) if link.is_some() => {
                let (href, label) = link.unwrap_or_default();
                self.open_link(&href, &label, cx)
            }
            (GestureKind::Tap, None) if tapped.is_some() => {
                if let Some(mark) = tapped {
                    book.tap_mark(words, mark, cx);
                }
                Handled::Yes
            }
            (GestureKind::LongPress, _) => match at(gesture.start) {
                Some(word) => {
                    self.set_selection(Some(Selection::touched(word)), cx);
                    Handled::Yes
                }
                None => Handled::No,
            },
            (GestureKind::Tap, Some(selection)) => {
                let items = self.items(&env, words);
                if let Some(item) = selection::item_at(&env, &items, gesture.start) {
                    self.activate(&item.id, cx);
                } else if let Some(word) = at(gesture.start) {
                    let to = Selection {
                        cursor: word,
                        mode: Mode::Bar,
                        ..selection
                    };
                    self.set_selection(Some(to), cx);
                } else {
                    self.set_selection(None, cx);
                }
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        self.event(event, params, cx);
        self.let_go_of_other_pages(cx);
    }
}

impl ReaderScreen {
    /// The page, its marks and status lines, and what is chosen on it.
    fn draw_book(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = UiStyle::new(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        let book = match &mut self.book {
            Ok(book) => book,
            Err(message) => {
                let area = Rect::from_size(env.device().display.width, env.device().display.height);
                return ui.message(area, message);
            }
        };
        // Word boxes are measured only when something is drawn on words.
        let has_highlights = book.marks.iter().any(|m| m.kind == Kind::Highlight);
        let measured = has_highlights
            || self.selection.is_some()
            || self.linking.is_some()
            || book.found.is_some()
            || self.aloud.is_some()
            || !book.links.is_empty()
            || !env.decorators().is_empty();
        let layered = !env.page_layers().is_empty();
        book.update_decorations(env);
        let Ok(book) = &self.book else {
            return;
        };
        let words = if measured { book.words(env) } else { &[] };
        // The sentence read aloud, where it is on this page.
        let aloud = self
            .aloud
            .filter(|a| {
                a.now.page == book.page.start
                    && env.settings().read(&crate::settings::ALOUD_HIGHLIGHT)
            })
            .map(|a| (a.now.from, a.now.to));
        let marks = book.page_marks(env, aloud, words);
        // The plugins' layers behind the text and over it; their line
        // boxes only while there are layers.
        let lines = if layered { book.lines() } else { &[] };
        // The page's words for a layer that draws by them, measured once
        // a page as for choosing them.
        let laid = match layered && env.page_layers().want_words(env) {
            true => book.words(env),
            false => &[],
        };
        let view = book.page_view(lines).words(laid);
        let rooms = &book.typography.rooms;
        if layered {
            env.page_layers().draw(ui.canvas(), env, &view, rooms, true);
        }
        marks.draw_behind(ui.canvas(), env);
        book.draw_page(ui.canvas(), env, &marks.inks);
        marks.draw_over(ui.canvas(), env);
        if layered {
            env.page_layers()
                .draw(ui.canvas(), env, &view, rooms, false);
        }
        book.draw_note_signs(&mut ui, words);
        book.draw_back(&mut ui);
        if let Some(drawn) = book.draw_status(ui.canvas(), env) {
            // Shown with the page: not refreshed again on their own.
            self.status_shown = Some(drawn);
        }
        if book.bookmarked() {
            book.draw_ribbon(ui.canvas(), env);
        }
        // A bar at the bottom hides the status line under it entirely.
        let bar = self.linking.is_some() || self.selection.is_some_and(|s| s.mode == Mode::Bar);
        if let Some(status) = book.typography.status.filter(|_| bar) {
            ui.canvas().fill(status, Color::WHITE);
        }
        if let Some(linking) = self.linking {
            draw_linking(&mut ui, book, words, linking);
        }
        let Some(selection) = self.selection else {
            return;
        };
        let (lo, hi) = selection.range();
        let line = env.metrics().tenth_mm(4).max(2);
        for b in selection::line_boxes(words, |i| (lo..=hi).contains(&i)) {
            ui.canvas().stroke_rect(
                i32::from(b.x) - 4,
                i32::from(b.y) - 2,
                u32::from(b.width) + 8,
                u32::from(b.height) + 4,
                line,
                Color::BLACK,
            );
        }
        if selection.mode == Mode::Bar {
            let items = self.items(env, words);
            selection::draw_bar(&mut ui, &items, selection.focus);
        }
    }

    /// Lets go of text chosen on a page no longer shown: read aloud,
    /// skimmed or turned by itself, its words are those of another page.
    fn let_go_of_other_pages(&mut self, cx: &mut Cx<'_, '_>) {
        if let (Some(_), Ok(book)) = (self.selection, &self.book)
            && book.page.start != self.selected_on
        {
            self.set_selection(None, cx);
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        // The book waited for is there now: opened.
        if let Some(ready) = events::DOCUMENT_READY.read(event, params)
            && self
                .waiting
                .as_ref()
                .is_some_and(|w| w.get(params::PATH) == Some(ready.path.as_str()))
        {
            self.preparing = self.waiting.take();
            self.prepare(cx);
            return;
        }
        let Ok(book) = &mut self.book else {
            return;
        };
        // Asleep or restarting the device may lose its power: the position
        // first.
        if *event == events::SUSPEND || *event == events::STOPPING {
            book.save_due(cx, true);
        }
        // Another plugin asks for the page's words.
        if let Some(wanted) = events::PAGE_WORDS_WANTED.read(event, params) {
            let (start, end) = (book.page.start, book.page.end);
            let words = paperpaw_sdk::reading::PageWords::new(
                book.path.clone(),
                book.language.clone(),
                start,
                end,
            )
            .words(page_words(&book.page.lines));
            cx.command(Command::Emit(wanted.reply, words.to_params()));
            return;
        }
        let this_book = params.get(params::PATH) == Some(book.path.as_str());
        if let Some(spoken) = crate::SPOKEN.read(event, params)
            && this_book
        {
            self.spoken(&spoken, cx);
            return;
        }
        // Changed elsewhere too, e.g. by a sync.
        if events::ANNOTATIONS_CHANGED
            .read(event, params)
            .is_some_and(|a| a.path == book.path)
        {
            // Those added there are taken as of this conversion.
            if book.reload_marks(cx.env()) {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            return;
        }
        if let Some(link) = events::FOLLOW_LINK.read(event, params)
            && link.path == book.path
        {
            self.follow(&link.href, cx);
            return;
        }
        if let Some(goto) = events::GOTO.read(event, params)
            && goto.path == book.path
        {
            // Another page: no text stays selected.
            self.selection = None;
            let env = *cx.env();
            let at = goto.at();
            let start = book.page_start(&env, at);
            book.jump(cx, start);
            if let Some(end) = goto.end() {
                book.found = Some((at, end));
                cx.request_redraw();
            }
            return;
        }
        if let Some(skim) = events::SKIM.read(event, params)
            && skim.path == book.path
        {
            self.skim(&skim.to, cx);
            return;
        }
        if let Some(typed) = crate::NOTE_ENTERED.read(event, params)
            && this_book
            && let Some(at) = crate::toc::locator_of(&typed.context)
        {
            let env = *cx.env();
            let note = typed.text.as_str();
            let now = paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0);
            if !matches!(
                annotations::set_note(env.storage(), &book.path, at, note, now),
                Ok(true)
            ) {
                marks_not_saved(cx);
            }
            book.marks = annotations::marks(env.storage(), &book.path);
            cx.command(annotations::changed(&book.path));
            cx.request_redraw();
            return;
        }
        if let Some(typed) = crate::SEARCH_ENTERED.read(event, params)
            && this_book
        {
            let text = typed.text.trim();
            if !text.is_empty() {
                self.last_search = text.to_string();
                cx.command(Command::OpenSlot(
                    slots::SEARCH,
                    Params::new()
                        .with(params::PATH, book.path.clone())
                        .with(params::TEXT, text.to_string()),
                ));
            }
            return;
        }
        if let Some(typed) = crate::PAGE_ENTERED.read(event, params)
            && this_book
        {
            let env = *cx.env();
            let number = typed.text.trim().parse::<u32>().ok();
            if let (Some(number), Some(index)) = (number, &book.index)
                && index.pages() > 0
            {
                let page = number.clamp(1, index.pages()) - 1;
                if let Some(start) = index.entry(env.storage(), page) {
                    self.selection = None;
                    book.jump(cx, start);
                }
            }
            return;
        }
        if let Some(typed) = crate::PERCENT_ENTERED.read(event, params)
            && this_book
        {
            let env = *cx.env();
            // 100 % is the last page.
            let permille = typed
                .text
                .trim()
                .parse::<u32>()
                .ok()
                .map(|p| (p * 10).min(999));
            if let Some(target) =
                permille.and_then(|p| book.skim_target(&env, &format!("permille:{p}")))
            {
                self.selection = None;
                book.jump(cx, target);
            }
            return;
        }
        // Which setting changed, if one did.
        let changed = events::SETTING_CHANGED.read(event, params).map(|c| c.id);
        let id = changed.as_ref().map_or("", Id::as_str);
        // The kernel's settings the page's type depends on (the reader's
        // own all begin with `reader.`).
        let font_changed = TYPE_SETTINGS.contains(&id)
            || id == paperpaw_sdk::builtin::settings::READING_FONT.as_str()
            || id == crate::settings::PUBLISHER_STYLES.as_str()
            // Anything else the page's box depends on: the frame, the
            // room a plugin's layer takes.
            || changed.is_some() && self.book.as_ref().is_ok_and(|b| b.retyped(cx.env()));
        // A layer switched on or off: drawn again.
        if changed.is_some() && cx.env().page_layers().get_by_switch(id).is_some() && !font_changed
        {
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
            return;
        }
        // How marks look, or what decorators mark (a switch, their data):
        // the page drawn again as it is laid out.
        if changed.is_some()
            && let Ok(book) = &self.book
        {
            let env = *cx.env();
            let look = env
                .settings()
                .specs()
                .any(|s| s.id.as_str() == id && s.group == paperpaw_sdk::builtin::groups::MARKS);
            let marked = book
                .decorated_for
                .is_some_and(|(_, signature)| signature != env.decorators().signature(&env))
                || book.decorated_for.is_none() && !env.decorators().is_empty();
            if look || marked {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                return;
            }
        }
        // How the letters are inked: the same page drawn again, in grays
        // or not from now on (the kernel's settings, so before those).
        if id == crate::settings::SMOOTH_TEXT.as_str()
            || id == crate::settings::TEXT_CONTRAST.as_str()
        {
            self.smooth = cx.env().settings().read(&crate::settings::SMOOTH_TEXT);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Full);
            return;
        }
        if *event == events::STATUS_CHANGED
            || changed.is_some() && !font_changed && id.starts_with("core.")
        {
            // The battery, the clock format, the time zone, …
            self.status_shown = None;
            self.update_status(cx);
            return;
        }
        let Ok(book) = &mut self.book else {
            return;
        };
        // The book's fonts are loaded with it.
        let reopen = id == crate::settings::PUBLISHER_STYLES.as_str()
            || id == paperpaw_sdk::builtin::settings::READING_FONT.as_str();
        if changed.is_some() && reopen {
            // Another conversion of the book: open it again where it was,
            // kept with its XPointer to find it in the other.
            book.save_position(cx, true);
            let env = *cx.env();
            let path = book.path.clone();
            self.selection = None;
            self.book = open_book(&env, &Params::new().with(params::PATH, path)).0;
            cx.request_redraw();
            cx.wake_at(env.now());
            return;
        }
        // Only a setting changes how the pages are laid out: the filters
        // are not asked for other events.
        if changed.is_none() {
            return;
        }
        let affects_layout = font_changed
            || id.starts_with("reader.") && id != crate::settings::OPEN_LAST.as_str()
            || env_filters_changed(cx.env(), book);
        if affects_layout {
            self.selection = None;
            let env = *cx.env();
            book.relayout(&env);
            cx.request_redraw();
            let now = env.now();
            cx.wake_at(now);
        }
    }
}

/// The words of `lines` with their places, from the lines (nothing
/// measured); a word broken at the end of a line is its two parts, each
/// with the whole word as its text (see `reading::Word`).
pub(super) fn page_words(lines: &[PageLine]) -> Vec<paperpaw_sdk::reading::Word> {
    use paperpaw_sdk::reading::Word;
    let mut words: Vec<Word> = Vec::new();
    for line in lines {
        let parts = line.text.split(' ').filter(|part| !part.is_empty());
        for (part, &(start, end)) in parts.zip(&line.words) {
            // Without the controls an EPUB's `dir` puts in, which draw
            // nothing: a decorator looks for the word itself.
            let part: String = part
                .chars()
                .filter(|&c| !paperpaw_sdk::text::is_invisible(c))
                .collect();
            if part.is_empty() {
                continue;
            }
            let at = |offset| Locator::new(line.section, offset);
            words.push(Word::new(part, at(start), at(end)));
        }
    }
    for i in 1..words.len() {
        let (first, next) = (&words[i - 1], &words[i]);
        if !first.text.ends_with('-') || next.start != first.end {
            continue;
        }
        // The layout's hyphen where the word is shorter in the book.
        let in_book = first.end.offset.saturating_sub(first.start.offset) as usize;
        let head = match in_book < first.text.len() {
            true => &first.text[..first.text.len() - 1],
            false => first.text.as_str(),
        };
        let whole = format!("{head}{}", next.text);
        words[i - 1].text.clone_from(&whole);
        words[i].text = whole;
    }
    words
}

/// The text of `lines` as the book has it: a word broken at the end of
/// a line is whole again, without the hyphen the layout put there (a
/// hyphen of the book's own stays).
pub(super) fn page_text(lines: &[PageLine]) -> String {
    let mut out = String::new();
    let mut joined = false;
    for (i, line) in lines.iter().enumerate() {
        // What a filter inserted is not the book's.
        if line.inserted {
            continue;
        }
        if i > 0 && !joined && !out.is_empty() {
            out.push(' ');
        }
        let last = line.words.last().copied();
        let next = lines.get(i + 1).filter(|n| n.section == line.section);
        let first = next.and_then(|n| n.words.first().copied());
        // The word goes on in the next line, at once.
        joined = line.text.ends_with('-')
            && matches!((last, first), (Some((_, end)), Some((start, _))) if start == end);
        let mut text = line.text.as_str();
        if let Some((start, end)) = last.filter(|_| joined) {
            let word = text.rsplit(' ').next().unwrap_or(text);
            if (end.saturating_sub(start) as usize) < word.len() {
                text = &text[..text.len() - 1];
            }
        }
        out.push_str(text);
    }
    out
}

/// Says that a bookmark, highlight or note could not be kept: the book's
/// annotations could not be read or written.
pub(super) fn marks_not_saved(cx: &mut Cx<'_, '_>) {
    let text = cx.env().i18n().tr("reader.marks-not-saved").to_string();
    cx.command(Command::Notify(text));
}

/// The book of `params` opened, or what to show instead; and whether it
/// is only not there yet.
fn open_book(env: &Env<'_>, params: &Params) -> (Result<Book, String>, bool) {
    match Book::open(env, params) {
        Ok(book) => (Ok(book), false),
        Err(unopened) => {
            let not_ready = unopened.not_ready();
            (Err(unopened.message()), not_ready)
        }
    }
}

/// The page turn an arrow stands for in a book: left and up back, right
/// and down on.
fn page_turn(action: &Id) -> Option<Id> {
    if *action == actions::LEFT || *action == actions::UP {
        Some(actions::PREV_PAGE)
    } else if *action == actions::RIGHT || *action == actions::DOWN {
        Some(actions::NEXT_PAGE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    /// The position as version 1 wrote it stays readable (see
    /// `docs/data-files.md`).
    #[test]
    fn positions_of_version_1_are_read() {
        assert_eq!(
            paperpaw_sdk::positions::Position::parse(include_str!(
                "../../testdata/progress.ini.v1"
            ))
            .map(|p| p.at),
            Some(paperpaw_sdk::document::Locator::new(7, 1234))
        );
    }

    #[test]
    fn chapters_in_parts_get_ticks_while_they_leave_room() {
        use super::draw::chapter_ticks;
        // Two parts of three chapters each.
        let book = [
            (0, 0),
            (1, 10),
            (1, 200),
            (1, 400),
            (0, 500),
            (1, 510),
            (1, 700),
        ];
        assert_eq!(chapter_ticks(&book), [0, 10, 200, 400, 500, 510, 700]);
        // Too many chapters for the bar: the parts only.
        let mut many = alloc::vec![(0, 0), (0, 500)];
        many.extend((1..200).map(|i| (1, i * 5)));
        many.sort_by_cached_key(|&(_, at)| at);
        assert_eq!(chapter_ticks(&many), [0, 500]);
        assert!(chapter_ticks(&[]).is_empty());
        // Many chapters without parts: each its tick, along the whole bar.
        let flat: Vec<(u8, u32)> = (0..120).map(|i| (0, i * 8)).collect();
        let ticks = chapter_ticks(&flat);
        assert_eq!(ticks.len(), 120);
        assert_eq!(ticks.last(), Some(&952));
    }
}
