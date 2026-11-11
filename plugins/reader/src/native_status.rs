//! The status lines of the reading screen (R2b-2): a line above the text and
//! one below, each switching between the lines the user chose (a tap shows the
//! next; the choice is kept, `choice`), their room (`room`: a place's height
//! depends on the status text size alone, so switching lines never changes the
//! pages), the reader's own items (title, chapter, page, progress: the host
//! never calls its caller back for those, so the values are made here), and the
//! drawing: the items line through the kit's `status_line` (other plugins'
//! items come composed by the host), the progress bar (`bar`), a line of
//! another plugin composed whole, or nothing.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error, Host,
        callbacks::DrawFrame,
        fonts::{Font, FontRole, TextOrigin},
        formatting::Arg,
        settings::{self, SettingValue},
        snapshot::Snapshot,
        ui::Ui,
    },
};
use paperpaw_ui_kit::{
    Messages, Style,
    mem::{self, SERVICE_BUFFER},
    paint::{self, Paint},
    status_line::{self, Face, Lists, Placed},
    text,
    widget::text::ink_baseline,
};

pub mod bar;
pub mod choice;
pub mod room;

pub use room::Room;

/// The lines of the reader: the three-part line of items (the kernel's id and
/// the reader's own from before it), the progress bar and the empty line.
pub const ITEMS_LINES: [&str; 2] = ["core.status-line.items", "reader.status-line.items"];
pub const PROGRESS_LINE: &str = "reader.status-line.progress";
pub const EMPTY_LINE: &str = "reader.status-line.empty";

/// The reader's own items.
pub mod items {
    pub const TITLE: &str = "reader.status.title";
    /// The chapter, or the title in a book with no table of contents.
    pub const CHAPTER: &str = "reader.status.chapter";
    pub const PAGE: &str = "reader.status.page";
    pub const PROGRESS: &str = "reader.status.progress";
}

/// The settings (the ids the declarations give them).
pub const SIZE: &str = "reader.status.size";
pub const EDGE: &str = "reader.status.edge";
const LINES: [&str; 2] = ["reader.status.top-lines", "reader.status.bottom-lines"];
const PARTS: [[&str; 3]; 2] = [
    [
        "reader.status.top-left",
        "reader.status.top-center",
        "reader.status.top-right",
    ],
    [
        "reader.status.bottom-left",
        "reader.status.bottom-center",
        "reader.status.bottom-right",
    ],
];
/// What the figures of a line are centred by.
const FIGURES: &str = "0123456789";
/// The most of a setting's text that is read.
const SETTING_MAX: usize = 1024;

fn text_of<'b>(host: &Host<'_>, key: &str, bytes: &'b mut [u8]) -> Result<&'b str, Error> {
    match settings::read_setting(host, key, bytes)? {
        SettingValue::Text(text) => Ok(text),
        _ => Ok(""),
    }
}

fn number(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut bytes = [0; 32];
    match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => Ok(value),
        _ => Ok(0),
    }
}

/// Whether the lines setting `key` lists a line. A list too long to be read
/// has one.
fn has_lines(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut bytes = [0; 256];
    match text_of(host, key, &mut bytes) {
        Ok(list) => Ok(choice::rotation(list).next().is_some()),
        Err(Error::NoSpace) => Ok(true),
        Err(error) => Err(error),
    }
}

/// The room of the status lines now, for a display of `dpi`.
///
/// # Errors
/// The host's refusal to read a setting.
pub fn room(host: &Host<'_>, dpi: u16) -> Result<Room, Error> {
    Ok(Room::new(
        has_lines(host, LINES[0])?,
        has_lines(host, LINES[1])?,
        (number(host, SIZE)?, number(host, EDGE)?),
        dpi,
    ))
}

/// The lists of the lines and of the parts of the line of items, as the
/// settings say them now.
#[derive(Debug, Default)]
pub struct Config {
    /// The lines to switch between, top then bottom.
    pub lines: [String; 2],
    /// What the line of items shows in each place.
    pub parts: [Lists; 2],
}

impl Config {
    /// Reads the settings.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals; a list too long to be read
    /// is read as none.
    pub fn read(host: &Host<'_>) -> Result<Self, Error> {
        let mut bytes = mem::buffer(SETTING_MAX)?;
        let mut get = |key: &str| match text_of(host, key, &mut bytes) {
            Ok(list) => text::copy(list),
            Err(Error::NoSpace) => Ok(String::new()),
            Err(error) => Err(error),
        };
        let mut config = Self::default();
        for (at, key) in LINES.iter().enumerate() {
            config.lines[at] = get(key)?;
        }
        for (at, keys) in PARTS.iter().enumerate() {
            config.parts[at] = Lists {
                left: get(keys[0])?,
                center: get(keys[1])?,
                right: get(keys[2])?,
                line: String::new(),
            };
        }
        Ok(config)
    }
}

/// What the book says to the reader's own items.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts<'a> {
    pub title: &'a str,
    /// The chapter's title; none until the contents say it.
    pub chapter: Option<&'a str>,
    /// How far into the book the page is, in millionths; none where unknown.
    pub ppm: Option<u32>,
    /// The page and the pages of the book, once counted.
    pub page: Option<(u32, u32)>,
    /// Where chapters begin, in thousandths of the book.
    pub marks: &'a [u16],
}

/// The whole percent of `ppm` millionths (as Beta 3: the part read, cut).
#[must_use]
pub const fn percent(ppm: u32) -> u32 {
    ppm / 10_000
}

/// The texts of the reader's own items, made for the page on show.
#[derive(Debug, Default)]
pub struct Texts {
    title: String,
    chapter: String,
    page: String,
    progress: String,
}

impl Texts {
    /// The texts of `facts`: the page as "12 / 340" once the pages are counted,
    /// else as a percentage; the progress as a percentage.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn new(host: &Host<'_>, facts: &Facts<'_>) -> Result<Self, Error> {
        let mut say = Messages::new()?;
        let progress = match facts.ppm {
            Some(ppm) => say.format(
                host,
                "reader.percent",
                &[Arg::Int {
                    name: "percent",
                    value: i64::from(percent(ppm)),
                }],
            )?,
            None => String::new(),
        };
        let page = match facts.page {
            Some((page, pages)) => say.format(
                host,
                "reader.page",
                &[
                    Arg::Int {
                        name: "page",
                        value: i64::from(page),
                    },
                    Arg::Int {
                        name: "pages",
                        value: i64::from(pages),
                    },
                ],
            )?,
            // Pages not counted yet: the part of the book read instead.
            None => text::copy(&progress)?,
        };
        let chapter = text::copy(
            facts
                .chapter
                .filter(|c| !c.is_empty())
                .unwrap_or(facts.title),
        )?;
        Ok(Self {
            title: text::copy(facts.title)?,
            chapter,
            page,
            progress,
        })
    }

    /// The items with their texts, as the line's layout takes them.
    #[must_use]
    pub fn own(&self) -> [(&str, &str); 4] {
        [
            (items::TITLE, &self.title),
            (items::CHAPTER, &self.chapter),
            (items::PAGE, &self.page),
            (items::PROGRESS, &self.progress),
        ]
    }
}

/// The lines chosen last and whether they were read yet.
#[derive(Debug, Default)]
pub struct State {
    chosen: [String; 2],
    loaded: bool,
}

impl State {
    /// The choice kept is read once.
    pub fn load(&mut self, storage: &dyn paperpaw_sdk::Storage) -> Result<(), Error> {
        if !self.loaded {
            self.chosen = choice::load(storage)?;
            self.loaded = true;
        }
        Ok(())
    }

    /// The line showing at the top (or bottom) among `list`.
    #[must_use]
    pub fn showing<'l>(&self, top: bool, list: &'l str) -> &'l str {
        choice::current(list, &self.chosen[usize::from(!top)])
    }

    /// The next line is shown at the top (or bottom) and the choice kept.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn advance(
        &mut self,
        top: bool,
        list: &str,
        storage: &dyn paperpaw_sdk::Storage,
    ) -> Result<(), Error> {
        let at = usize::from(!top);
        let next = text::copy(choice::next(list, &self.chosen[at]))?;
        self.chosen[at] = next;
        choice::save(storage, &self.chosen)
    }
}

/// Where the lines are on the display: the room, the text's left edge and
/// width, and the display.
#[derive(Clone, Copy, Debug)]
pub struct Place<'d> {
    pub room: Room,
    /// The strips' left edge and width.
    pub across: (u16, u16),
    pub display: &'d Snapshot,
}

impl Place<'_> {
    fn strip(&self, top: bool) -> Option<Rect> {
        self.room
            .strip(top, self.across, self.display.display_height())
    }
}

/// Which line a strip shows.
enum Line<'l> {
    Items,
    Progress,
    Empty,
    /// A line of another plugin, composed whole.
    Other(&'l str),
}

fn kind(id: &str) -> Option<Line<'_>> {
    match id {
        "" => None,
        id if ITEMS_LINES.contains(&id) => Some(Line::Items),
        PROGRESS_LINE => Some(Line::Progress),
        EMPTY_LINE => Some(Line::Empty),
        other => Some(Line::Other(other)),
    }
}

/// Draws the lines whose strips are in the rows the frame may draw: for each,
/// its paint is replayed first, then the items and lines others draw are
/// composed over it.
///
/// # Errors
/// [`Error::NoMemory`], font and host refusals. A host that does not answer the
/// status calls leaves the line empty.
pub fn paint(
    host: &Host<'_>,
    ui: &Ui,
    frame: &mut DrawFrame<'_>,
    (place, state, facts): (&Place<'_>, &State, &Facts<'_>),
) -> Result<(), Error> {
    let (from, to) = (frame.clip_top, frame.clip_bottom);
    let wanted = |top: bool| {
        place
            .strip(top)
            .filter(|s| i32::from(s.y) < to && i32::from(s.y) + i32::from(s.height) > from)
    };
    let strips = [wanted(true), wanted(false)];
    if strips.iter().all(Option::is_none) {
        return Ok(());
    }
    let config = Config::read(host)?;
    let texts = Texts::new(host, facts)?;
    let font = Font::acquire_role(host, FontRole::Status)?;
    let drawn = paint_each(
        host,
        ui,
        frame,
        &font,
        (place, &config, state, (&texts, facts)),
        strips,
    );
    let released = font.release();
    drawn.and(released)
}

fn paint_each(
    host: &Host<'_>,
    ui: &Ui,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    (place, config, state, (texts, facts)): (&Place<'_>, &Config, &State, (&Texts, &Facts<'_>)),
    strips: [Option<Rect>; 2],
) -> Result<(), Error> {
    let color = Style::read(host, place.display, font)?.note;
    for (at, strip) in strips.iter().enumerate() {
        let Some(strip) = *strip else { continue };
        let showing = state.showing(at == 0, &config.lines[at]);
        let ink = (strip, color);
        paint_one(
            host,
            ui,
            frame,
            font,
            (ink, place.room.size),
            (&config.parts[at], showing, (texts, facts)),
        )?;
    }
    Ok(())
}

fn paint_one(
    host: &Host<'_>,
    ui: &Ui,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    ((strip, color), size): ((Rect, Color), u16),
    (parts, showing, (texts, facts)): (&Lists, &str, (&Texts, &Facts<'_>)),
) -> Result<(), Error> {
    let size = f32::from(size);
    let mut ops = Vec::new();
    paint::push(&mut ops, Paint::Fill(strip, Color::WHITE))?;
    let mut placed: Vec<Placed<'_>> = Vec::new();
    let mut buffers = [Vec::new(), Vec::new()];
    let face = Face {
        host,
        font,
        small: size,
    };
    let own = texts.own();
    let wholly = Lists {
        line: kind_other(showing)?,
        ..Lists::default()
    };
    let lists = match kind(showing) {
        Some(Line::Items) => Some(parts),
        Some(Line::Other(_)) => Some(&wholly),
        _ => None,
    };
    if let Some(lists) = lists {
        if lists.line.is_empty() {
            buffers = [mem::buffer(SERVICE_BUFFER)?, mem::buffer(SERVICE_BUFFER)?];
        }
        let drawn = status_line::draw_own(
            (&face, lists),
            ui,
            (&mut ops, &mut placed),
            &mut buffers,
            ((strip, color), &own),
        );
        // A host that does not answer the status calls leaves the line empty.
        match drawn {
            Err(Error::Unsupported) => {}
            other => other?,
        }
    } else if matches!(kind(showing), Some(Line::Progress)) {
        progress(&mut ops, font, ((strip, size, color), (texts, facts)))?;
    }
    paint::replay(frame, font, ops.iter())?;
    for item in placed {
        // An item or line the host cannot compose is left out.
        Ui::compose(host, item.target, item.rect).ok();
    }
    Ok(())
}

/// The id of a line of another plugin, or empty.
fn kind_other(id: &str) -> Result<String, Error> {
    match kind(id) {
        Some(Line::Other(id)) => text::copy(id),
        _ => Ok(String::new()),
    }
}

/// The progress bar and the page's label on its right.
fn progress(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    ((strip, size, color), (texts, facts)): ((Rect, f32, Color), (&Texts, &Facts<'_>)),
) -> Result<(), Error> {
    let label = texts.page.as_str();
    let label_width = if label.is_empty() {
        0.0
    } else {
        font.measure_text(size, label)?
    };
    let narrow = u16::try_from((label_width + 1.0) as u32).unwrap_or(u16::MAX);
    let ppm = facts.ppm.unwrap_or(0);
    for rect in bar::rects(strip, size as u16, narrow, ppm, facts.marks)? {
        paint::push(ops, Paint::Fill(rect, color))?;
    }
    if !label.is_empty() {
        let baseline = ink_baseline(font, size, FIGURES, strip)?;
        let x = f32::from(strip.x) + f32::from(strip.width) - label_width;
        paint::push(
            ops,
            Paint::Text {
                text: text::copy(label)?,
                size,
                origin: TextOrigin::new(x, baseline),
                color,
                clip: Some(strip),
            },
        )?;
    }
    Ok(())
}

/// What a tap did.
#[derive(Debug, PartialEq, Eq)]
pub enum Tapped {
    /// Not on a status line.
    Missed,
    /// On the line at the top (`true`) or bottom: the next line shows.
    Next(bool),
    /// On an item that does something: its action, to be run.
    Action(String),
}

/// What a tap at `point` does: on an item of the items line with an action of
/// its owner's that action, elsewhere on a line the next line.
///
/// # Errors
/// [`Error::NoMemory`], font and host refusals.
pub fn tap(
    host: &Host<'_>,
    ui: &Ui,
    point: Point,
    (place, state, facts): (&Place<'_>, &State, &Facts<'_>),
) -> Result<Tapped, Error> {
    let display = (
        place.display.display_width(),
        place.display.display_height(),
    );
    let hit = [true, false].into_iter().find(|top| {
        place
            .room
            .zone(*top, display)
            .is_some_and(|zone| zone.contains(point))
    });
    let Some(top) = hit else {
        return Ok(Tapped::Missed);
    };
    let Some(strip) = place.strip(top) else {
        return Ok(Tapped::Missed);
    };
    let config = Config::read(host)?;
    let at = usize::from(!top);
    let showing = state.showing(top, &config.lines[at]);
    if !matches!(kind(showing), Some(Line::Items)) {
        return Ok(Tapped::Next(top));
    }
    let texts = Texts::new(host, facts)?;
    let font = Font::acquire_role(host, FontRole::Status)?;
    let found = item_action(
        host,
        ui,
        &font,
        (strip, place.room.size, point.x),
        (&config.parts[at], &texts),
    );
    let released = font.release();
    let found = found?;
    released?;
    Ok(found.map_or(Tapped::Next(top), Tapped::Action))
}

/// The action of the item under `x` in the items line, where there is one.
fn item_action(
    host: &Host<'_>,
    ui: &Ui,
    font: &Font<'_>,
    (strip, size, x): (Rect, u16, i32),
    (parts, texts): (&Lists, &Texts),
) -> Result<Option<String>, Error> {
    let face = Face {
        host,
        font,
        small: f32::from(size),
    };
    let own = texts.own();
    let hit = match status_line::targets_own((&face, parts), ui, strip, &own) {
        Ok(targets) => targets.into_iter().find(|(_, rect)| {
            (i32::from(rect.x)..i32::from(rect.x) + i32::from(rect.width)).contains(&x)
        }),
        Err(Error::Unsupported) => None,
        Err(error) => return Err(error),
    };
    match hit {
        Some((target, _)) => status_line::action_own((&face, parts), ui, (strip, &own), target),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests;
