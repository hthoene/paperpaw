//! The quote of the day: a quote about reading, books and life, another
//! each day, in the reader's language, from those PaperPaw brings (in
//! `data/quotes.<language>.txt`) and the reader's own (`quotes.txt` in
//! the plugin's folder, a quote per line: `text | author`). A tap shows
//! the next one; set to change each time it shows, the showings are
//! counted in memory ([`Showings`]).

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::files::for_each_line;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec};

use super::text::{self, FACES};
use super::{Changes, Turns, data, frame_height, of_the_day, today};
use crate::showings::Showings;

pub(super) const ID: Id = Id::new_static("widgets.quote");
/// Whose quotes: one of [`SOURCES`].
const SOURCE: Setting<Text> = Setting::new("widgets.quote.source");
/// PaperPaw's, the reader's own, or both.
const SOURCES: [&str; 3] = [
    "widgets.quote.source.shipped",
    "widgets.quote.source.own",
    "widgets.quote.source.both",
];
/// When another quote shows: one of [`CHANGES`].
const CHANGE: Setting<Text> = Setting::new("widgets.quote.change");
/// Each day, or each time the widget shows.
pub(super) const CHANGES: [&str; 2] = ["widgets.change.daily", "widgets.change.shown"];
/// The font of the quote: one of [`FACES`].
const FACE: Setting<Text> = Setting::new("widgets.quote.face");
/// Parts it shows: who said it, quotation marks around it, a large
/// quotation mark above it, "Quote of the day" above all.
const AUTHOR: &str = "author";
const MARKS: &str = "marks";
const MARK: &str = "mark";
const CAPTION: &str = "caption";
/// The next quote, asked for with a tap.
const NEXT: Event<()> = Event::new("widgets.quote.next");

/// Where PaperPaw's quotes are on the card, installed with the plugin:
/// `quotes.<language>.txt`, a quote per line, as the reader's own are.
const FOLDER: &str = "plugins/widgets/data";

/// The same quotes carried in the code, for where the plugin is part of
/// the app (a shell with room: there is no installed folder to read
/// from). A plugin file for a reader leaves them out
/// (`default-features = false` in tools/plugin-files): its package brings
/// them on the card.
#[cfg(feature = "embedded-quotes")]
const ENGLISH: &str = include_str!("../../data/quotes.en.txt");
#[cfg(feature = "embedded-quotes")]
const GERMAN: &str = include_str!("../../data/quotes.de.txt");

/// Where the reader's own quotes are.
pub(super) fn own_file() -> String {
    data("quotes.txt")
}

/// Where the turns of the quotes are kept (see [`Turns`]).
fn turns_file() -> String {
    data("quote.txt")
}

/// The quote's settings, kept in the program.
static SETTINGS: [SettingSpec; 13] = [
    SettingSpec::choice(
        "widgets.quote.source",
        "widgets.quote.setting.source",
        &SOURCES,
        SOURCES[2],
        ID,
    )
    .description("widgets.quote.setting.source.help"),
    SettingSpec::choice(
        "widgets.quote.change",
        "widgets.setting.change",
        &CHANGES,
        CHANGES[0],
        ID,
    )
    .description("widgets.setting.change.help"),
    widgets::part_spec(
        "widgets.quote.author",
        ID,
        "widgets.quote.setting.author",
        true,
    ),
    widgets::part_spec(
        "widgets.quote.marks",
        ID,
        "widgets.quote.setting.marks",
        true,
    ),
    widgets::part_spec(
        "widgets.quote.mark",
        ID,
        "widgets.quote.setting.mark",
        false,
    ),
    widgets::part_spec(
        "widgets.quote.caption",
        ID,
        "widgets.quote.setting.caption",
        false,
    ),
    SettingSpec::choice(
        "widgets.quote.face",
        "widgets.setting.font",
        &FACES,
        FACES[2],
        ID,
    )
    .description("widgets.setting.face-font.help"),
    widgets::size_spec("widgets.quote.size", ID, TextSize::Fit, true),
    widgets::align_spec("widgets.quote.align", ID, Align::Center),
    widgets::frame_spec("widgets.quote.frame", ID, Frame::None),
    widgets::padding_spec("widgets.quote.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.quote.paper", ID),
    widgets::ink_spec("widgets.quote.ink", ID),
];

pub(super) fn register(r: &mut Registrar, changes: &Changes) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.quote");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    let changed = changes.clone();
    r.on(NEXT, move |env, _, out| {
        Turns::turn(env.storage(), &turns_file(), today(env), daily(env));
        changed.changed(out, &ID);
    })?;
    let changes = changes.clone();
    let shown = Rc::new(Showings::default());
    r.add_widget(
        WidgetSpec::new(ID, "widgets.quote", move |env| {
            // Another quote each time it shows: made as it shows.
            let showing = if daily(env) { 0 } else { shown.show(env.now()) };
            Box::new(QuoteWidget {
                showing,
                changes: changes.clone(),
                ..QuoteWidget::default()
            })
        })
        .category(widgets::categories::WORDS),
    )
}

/// Whether another quote shows each day (else each time it shows).
fn daily(env: &Env<'_>) -> bool {
    env.settings().read(&CHANGE) != CHANGES[1]
}

/// A quote and who said it (empty if no one is known).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Quote {
    text: String,
    author: String,
}

/// A quote as a line of a file has it, `text | author`; `None` for
/// empty lines and comments (`#`).
fn parse(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (text, author) = line.rsplit_once('|').unwrap_or((line, ""));
    let text = text::unquoted(text);
    (!text.is_empty()).then_some((text, author.trim()))
}

/// Where a list of quotes is: carried in the code, or a file.
#[derive(Clone, Copy)]
enum Source<'a> {
    Text(&'a str),
    File(&'a str),
}

impl Source<'_> {
    /// Hands each line on until `each` breaks.
    fn lines(
        self,
        storage: &dyn paperpaw_sdk::Storage,
        each: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) {
        match self {
            Source::Text(text) => {
                for line in text.lines() {
                    if each(line).is_break() {
                        break;
                    }
                }
            }
            // No file: no quotes.
            Source::File(path) => {
                let _ = for_each_line(storage, path, each);
            }
        }
    }

    /// How many quotes there are.
    fn count(self, storage: &dyn paperpaw_sdk::Storage) -> usize {
        let mut count = 0;
        self.lines(storage, &mut |line| {
            count += usize::from(parse(line).is_some());
            ControlFlow::Continue(())
        });
        count
    }

    /// The quote after `skip` others.
    fn nth(self, storage: &dyn paperpaw_sdk::Storage, skip: usize) -> Option<Quote> {
        let mut found = None;
        let mut index = 0;
        self.lines(storage, &mut |line| {
            let Some((text, author)) = parse(line) else {
                return ControlFlow::Continue(());
            };
            if index == skip {
                found = Some(Quote {
                    text: String::from(text),
                    author: String::from(author),
                });
                return ControlFlow::Break(());
            }
            index += 1;
            ControlFlow::Continue(())
        });
        found
    }
}

/// The file of PaperPaw's quotes in `language` (German, else English) on
/// the card, if it is there.
fn card_file(storage: &dyn paperpaw_sdk::Storage, language: &str) -> Option<String> {
    let tongue = if language.starts_with("de") {
        "de"
    } else {
        "en"
    };
    let path = alloc::format!("{FOLDER}/quotes.{tongue}.txt");
    storage.exists(&path).then_some(path)
}

/// The quotes of `text`, the shipped ones' file.
#[cfg(test)]
fn quotes_in(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(parse)
}

/// The quote at `position` of the shipped ones (`shipped`) and then the
/// reader's own (in the file at `own`), and how many there are in all;
/// `None` without any.
fn quote_at(
    storage: &dyn paperpaw_sdk::Storage,
    shipped: Option<Source<'_>>,
    own: Option<&str>,
    position: impl Fn(usize) -> usize,
) -> Option<Quote> {
    let own = own.map(Source::File);
    let from_shipped = shipped.map_or(0, |s| s.count(storage));
    let from_own = own.map_or(0, |s| s.count(storage));
    let total = from_shipped + from_own;
    if total == 0 {
        return None;
    }
    let at = position(total) % total;
    if at < from_shipped {
        return shipped?.nth(storage, at);
    }
    own?.nth(storage, at - from_shipped)
}

/// The quotes PaperPaw brings in `language`: the card's file, else those
/// the code carries (English where it brings none); `None` for neither.
fn shipped<'a>(card: Option<&'a str>, language: &str) -> Option<Source<'a>> {
    if let Some(path) = card {
        return Some(Source::File(path));
    }
    carried(language).map(Source::Text)
}

#[cfg(feature = "embedded-quotes")]
fn carried(language: &str) -> Option<&'static str> {
    Some(if language.starts_with("de") {
        GERMAN
    } else {
        ENGLISH
    })
}

/// A plugin file for a reader carries none.
#[cfg(not(feature = "embedded-quotes"))]
fn carried(_language: &str) -> Option<&'static str> {
    None
}

/// Today's quote, turned on as often as tapped (and, where another
/// shows each time, as often as shown: `showing`).
fn quote_of_the_day(env: &Env<'_>, day: i64, showing: u32) -> Option<Quote> {
    let source = env.settings().read(&SOURCE);
    let daily = daily(env);
    let turns = Turns::load(env.storage(), &turns_file())
        .on(day, daily)
        .wrapping_add(showing);
    let card = card_file(env.storage(), env.i18n().language());
    let shipped = (source != SOURCES[1]).then(|| shipped(card.as_deref(), env.i18n().language()));
    let shipped = shipped.flatten();
    let own = own_file();
    let own = (source != SOURCES[0]).then_some(own.as_str());
    quote_at(env.storage(), shipped, own, |total| {
        let first = if daily { of_the_day(day, total) } else { 0 };
        first + turns as usize
    })
}

/// The quote of the day, read when updated.
#[derive(Default)]
struct QuoteWidget {
    /// The showing it was made for, where another shows each time.
    showing: u32,
    changes: Changes,
    quote: Option<Quote>,
    /// The day and the changes it was read at.
    read: Option<(i64, u32)>,
    /// Where it is drawn, once placed, and the quote laid out there.
    area: Option<Rect>,
    laid: Option<Laid>,
}

/// A quote laid out in a widget's area, drawn as it is in every band.
#[derive(Debug, Clone, PartialEq)]
struct Laid {
    /// The caption shows above it.
    caption: bool,
    /// Where it goes below the caption, and how large.
    area: Rect,
    size: f32,
    /// The large mark above, and who said it below, if they show.
    layout: Layout,
    author: Option<String>,
    lines: Vec<String>,
    /// Where the block begins from the top.
    top: u16,
}

/// `quote` laid out in widget area `area`: as large as its size setting
/// says (or as fits); where a line of it would not fit with them, the
/// large mark and who said it go.
fn lay_out(env: &Env<'_>, area: Rect, quote: &Quote) -> Option<Laid> {
    let look = widgets::widget_look(env, ID.as_str(), area);
    let mut area = look.area;
    let caption = widgets::shows(env, ID.as_str(), CAPTION);
    if caption {
        let height = widgets::caption_height(env).min(area.height);
        area = Rect::new(area.x, area.y + height, area.width, area.height - height);
    }
    let font = text::font(env, env.settings().read(&FACE))?;
    let style = UiStyle::new(env);
    let language = env.i18n().language();
    let shown = if widgets::shows(env, ID.as_str(), MARKS) {
        text::quoted(&quote.text, language)
    } else {
        String::from(text::unquoted(&quote.text))
    };
    let author = (widgets::shows(env, ID.as_str(), AUTHOR) && !quote.author.is_empty())
        .then_some(quote.author.as_str());
    let width = f32::from(area.width);
    let height = f32::from(area.height);
    // Who said it, where a line of the quote has room above it (the
    // name shortened where it is wider than the place).
    let author_h = f32::from(widgets::line_height(env, style.small())) + style.gap() as f32 * 0.75;
    let least = style.small() * 0.85;
    let author = author.filter(|_| text::line_height(&font, least) + author_h <= height);
    let mut wished = Layout {
        mark: widgets::shows(env, ID.as_str(), MARK),
        author: author.map_or(0.0, |_| author_h),
    };
    if wished.height(&font, least, 1) > height {
        wished.mark = false;
    }
    // Where the quote does not fit whole with it, the large mark goes;
    // where not even so, the quote is shortened at its end.
    let plain = Layout {
        mark: false,
        ..wished
    };
    let choices = [wished, plain];
    let whole = |layout: &Layout, size: f32| {
        let (lines, too_wide) = text::count_lines(&font, size, &shown, width);
        !too_wide && layout.height(&font, size, lines) <= height
    };
    let (layout, size) = match widgets::size(env, ID.as_str()) {
        TextSize::Fit => choices
            .iter()
            .find_map(|layout| {
                widgets::largest_size(style.title() * 2.0, least, |size| whole(layout, size))
                    .map(|size| (*layout, size))
            })
            .unwrap_or((plain, least)),
        size => {
            let size = size.text(&style);
            let layout = choices.iter().find(|layout| whole(layout, size));
            (*layout.unwrap_or(&plain), size)
        }
    };
    let author = author.filter(|_| layout.author > 0.0);
    let line = text::line_height(&font, size);
    let room = f32::from(area.height) - layout.height(&font, size, 0);
    // As many lines as fit; none where not even one has room.
    let fits = (room / line).max(0.0) as usize;
    let lines = if fits == 0 {
        Vec::new()
    } else {
        text::lines(&font, size, &shown, width, fits)
    };
    let total = layout.height(&font, size, lines.len()) as u16;
    Some(Laid {
        caption,
        area,
        size,
        layout,
        author: author.map(String::from),
        lines,
        top: area.y + area.height.saturating_sub(total) / 2,
    })
}

impl QuoteWidget {
    fn lay_out(&mut self, env: &Env<'_>) {
        self.laid = match (self.area, &self.quote) {
            (Some(area), Some(quote)) => lay_out(env, area, quote),
            _ => None,
        };
    }
}

impl Widget for QuoteWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        // Read each minute, but read anew only on another day or after a
        // tap.
        let day = today(env);
        let read = Some((day, self.changes.count()));
        if read == self.read {
            return false;
        }
        self.read = read;
        let quote = quote_of_the_day(env, day, self.showing);
        let changed = quote != self.quote;
        self.quote = quote;
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

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let line = widgets::line_height(env, style.title());
        line * 3
            + widgets::line_height(env, style.small())
            + style.gap()
            + frame_height(env, ID.as_str())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let Some(quote) = &self.quote else {
            let message = env.i18n().tr("widgets.quote.none");
            super::draw_empty(canvas, env, &look, message);
            return;
        };
        // Laid out when placed; drawn elsewhere, laid out for that.
        let laid = match &self.laid {
            Some(laid) if self.area == Some(area) => Cow::Borrowed(laid),
            _ => match lay_out(env, area, quote) {
                Some(laid) => Cow::Owned(laid),
                None => return,
            },
        };
        if laid.caption {
            let caption = env.i18n().tr("widgets.quote.caption");
            widgets::draw_caption(canvas, env, ID.as_str(), &look, caption);
        }
        let Some(font) = text::font(env, env.settings().read(&FACE)) else {
            return;
        };
        let style = UiStyle::new(env);
        let mut ui = paperpaw_sdk::ui::Ui::new(canvas, env, &style);
        let language = env.i18n().language();
        let (area, size, layout) = (laid.area, laid.size, laid.layout);
        let width = f32::from(area.width);
        let mut top = laid.top;
        let align = widgets::align(env, ID.as_str());
        if layout.mark {
            // The opening mark large and faint above the quote, its top
            // where the block begins.
            let mark = ornament(language);
            let big = size * MARK_SCALE;
            let x = align.x(area, font.measure(big, mark));
            // Quotation marks are as high as capitals, a third of them.
            let baseline = f32::from(top) + big * 0.72;
            font.draw(ui.canvas(), big, x, baseline as i32, mark, look.faint);
            top += layout.mark_height(size) as u16;
        }
        top += text::draw_lines(
            ui.canvas(),
            &font,
            size,
            &laid.lines,
            area,
            top,
            align,
            look.ink,
        );
        if let (Some(author), Some(ui_font)) = (&laid.author, ui.font()) {
            top += (style.gap() as f32 * 0.75) as u16;
            let small = widgets::line_height(env, style.small());
            let row = Rect::new(area.x, top, area.width, small);
            // The whole name where it fits, else the last name alone
            // ("Bacon"), smaller where need be, then shortened.
            let whole = ui_font.measure(style.small(), author) <= width;
            let last = author.rsplit(' ').next().unwrap_or(author);
            let shown = if whole { author.as_str() } else { last };
            let (size, name) =
                widgets::fit_line(&ui_font, style.small(), style.small() * 0.85, shown, width);
            let x = align.x(area, ui_font.measure(size, &name));
            paperpaw_sdk::ui::Label::new(&name)
                .px(size)
                .color(look.faint)
                .draw_from(&mut ui, x, row);
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget::new(area, NEXT.emit(()))]
    }
}

/// How large the opening mark above the quote is, to the quote's size.
const MARK_SCALE: f32 = 2.4;

/// The large mark above a quote in `language`: a raised one, as low
/// opening marks look lost above the text; guillemets where the
/// language has them.
fn ornament(language: &str) -> &'static str {
    match text::marks(language).0.trim() {
        "«" | "»" => "«",
        _ => "“",
    }
}

/// What goes with the quote's lines.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// A large opening mark above.
    mark: bool,
    /// The height of who said it, below (0 if not shown).
    author: f32,
}

impl Layout {
    /// The room of the large mark above the quote at `size`: the upper
    /// part of a line, where quotation marks are.
    fn mark_height(&self, size: f32) -> f32 {
        if self.mark {
            size * MARK_SCALE * 0.42
        } else {
            0.0
        }
    }

    /// The height of it all with `lines` lines at `size`.
    fn height(&self, font: &paperpaw_sdk::FontHandle<'_>, size: f32, lines: usize) -> f32 {
        self.mark_height(size) + text::line_height(font, size) * lines as f32 + self.author
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn its_settings_are_those_read() {
        let ids: alloc::vec::Vec<&str> = super::SETTINGS.iter().map(|s| s.id.as_str()).collect();
        for read in [
            super::SOURCE.as_str(),
            super::CHANGE.as_str(),
            super::FACE.as_str(),
            "widgets.quote.author",
            "widgets.quote.marks",
            "widgets.quote.mark",
            "widgets.quote.caption",
        ] {
            assert!(ids.contains(&read), "{read}");
        }
        for look in ["size", "align", "frame"] {
            let id = alloc::format!("{}.{look}", super::ID);
            assert!(ids.contains(&id.as_str()), "{id}");
        }
        assert!(super::SETTINGS.iter().all(|s| s.group == super::ID));
    }
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn enough_quotes_come_with_paperpaw() {
        let english = include_str!("../../data/quotes.en.txt");
        let german = include_str!("../../data/quotes.de.txt");
        for (text, language) in [(english, "en"), (german, "de")] {
            let quotes: Vec<_> = quotes_in(text).collect();
            assert!(quotes.len() >= 60, "{language}: {}", quotes.len());
            for (quote, author) in &quotes {
                assert!(!author.is_empty(), "{quote}");
                // Marks come from the widget, as the language has them;
                // dashes are not used.
                assert!(!quote.starts_with(['"', '“', '„']), "{quote}");
                assert!(!quote.contains(['\u{2013}', '\u{2014}']), "{quote}");
            }
        }
    }

    /// The card's file of PaperPaw's quotes is the language's, German or
    /// else English; the code's are the same where the plugin carries them.
    #[test]
    fn the_quotes_come_from_the_card_or_the_code() {
        let english = include_str!("../../data/quotes.en.txt");
        let german = include_str!("../../data/quotes.de.txt");
        let storage = MemoryStorage::new()
            .with("plugins/widgets/data/quotes.en.txt", english)
            .with("plugins/widgets/data/quotes.de.txt", german);
        assert_eq!(
            card_file(&storage, "de-AT").as_deref(),
            Some("plugins/widgets/data/quotes.de.txt")
        );
        assert_eq!(
            card_file(&storage, "fr").as_deref(),
            Some("plugins/widgets/data/quotes.en.txt")
        );
        let path = card_file(&storage, "de").unwrap();
        let from_card = quote_at(&storage, shipped(Some(&path), "de"), None, |_| 0).unwrap();
        assert_eq!(
            from_card.text,
            quotes_in(german).next().map(|(t, _)| t).unwrap()
        );
        // Without the files: the code's, where it carries them, else none.
        let bare = MemoryStorage::new();
        assert_eq!(card_file(&bare, "en"), None);
        let carried = quote_at(&bare, shipped(None, "de"), None, |_| 0);
        if cfg!(feature = "embedded-quotes") {
            assert_eq!(carried.unwrap().text, from_card.text);
        } else {
            assert!(carried.is_none());
        }
    }

    #[test]
    fn own_quotes_are_parsed() {
        assert_eq!(parse("Read on. | Me"), Some(("Read on.", "Me")));
        assert_eq!(
            parse("  \"Quoted\" |  Someone  "),
            Some(("Quoted", "Someone"))
        );
        assert_eq!(parse("No one said it"), Some(("No one said it", "")));
        // The last bar parts the author.
        assert_eq!(parse("a | b | c"), Some(("a | b", "c")));
        assert_eq!(parse("# a comment | x"), None);
        assert_eq!(parse("   "), None);
        assert_eq!(parse(" | Nobody"), None);
    }

    #[test]
    fn quotes_come_from_the_sources_chosen() {
        let own = "# Mine\nFirst own | A\n\nSecond own\n";
        let storage = MemoryStorage::new().with(&own_file(), own);
        let path = own_file();
        let shipped = "One | X\nTwo | Y\n";
        let at = |shipped: Option<&str>, own: Option<&str>, n: usize| {
            quote_at(&storage, shipped.map(Source::Text), own, |_| n).map(|q| q.text)
        };
        // Shipped first, then the own ones, round again.
        let both: Vec<_> = (0..5)
            .map(|n| at(Some(shipped), Some(&path), n).unwrap())
            .collect();
        assert_eq!(
            both,
            ["One", "Two", "First own", "Second own", "One"].map(String::from)
        );
        assert_eq!(at(None, Some(&path), 1).as_deref(), Some("Second own"));
        assert_eq!(at(Some(shipped), None, 3).as_deref(), Some("Two"));
        let author = quote_at(&storage, None, Some(&path), |_| 0).unwrap().author;
        assert_eq!(author, "A");
        // None at all: none shown.
        assert_eq!(at(None, Some("missing.txt"), 0), None);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn own_quotes_of_version_1_are_read() {
        let storage =
            MemoryStorage::new().with(&own_file(), include_str!("../../testdata/quotes.txt.v1"));
        let path = own_file();
        let all: Vec<Quote> = (0..3)
            .map(|n| quote_at(&storage, None, Some(&path), |_| n).unwrap())
            .collect();
        assert_eq!(all[0].author, "Someone");
        assert_eq!(all[1].text, "Read on.");
        assert_eq!(all[2].text, "No one said this");
        assert_eq!(all[2].author, "");
    }

    #[test]
    fn a_quote_a_day() {
        let storage = MemoryStorage::new();
        let pick = |day: i64| {
            quote_at(
                &storage,
                Some(Source::Text(include_str!("../../data/quotes.en.txt"))),
                None,
                |total| of_the_day(day, total),
            )
            .unwrap()
        };
        assert_eq!(pick(20_375), pick(20_375));
        assert_ne!(pick(20_375), pick(20_376));
    }
}
