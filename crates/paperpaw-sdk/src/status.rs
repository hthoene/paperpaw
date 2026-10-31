//! Status items: small pieces of information shown in a status line, such
//! as the time, the battery or the page.
//!
//! Any plugin adds items with
//! [`Registrar::add_status_item`](crate::Registrar::add_status_item);
//! screens with a status line let the user choose which items it shows and
//! where (the reader does this for its lines above and below the text).
//! An item computes its value from the environment and from the context the
//! screen passes, e.g. the reader's `title`, `chapter`, `page` and
//! `progress` (see [`context`]).
//!
//! An item may do something when it is tapped ([`StatusItemSpec::tap`]:
//! the battery opens its settings), and says how much it matters
//! ([`StatusItemSpec::priority`]): where a line has no room for all its
//! items, those that matter least are left out first, never cut.
//!
//! Plugins can also add whole status lines
//! ([`Registrar::add_status_line`](crate::Registrar::add_status_line)),
//! which draw the line themselves: a progress bar, a large clock, anything.
//! The reader offers the lines the user chose, above and below the text,
//! and switches between them when the line is tapped.
//!
//! The places where status items show are [`Place`]s
//! ([`Registrar::add_status_place`](crate::Registrar::add_status_place)):
//! three settings of the items for a line's left, centre and right. The
//! reader has one above and one below the text, the home screen one in
//! its status widget, the quick panel one at its head; a status line
//! drawn in a place is told which ([`StatusFrame::place`]).

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use crate::env::Env;
use crate::fonts::FontHandle;
use crate::id::Id;
use crate::params::Params;

/// Parameters screens pass to status items. Items ignore what they do not
/// need; a screen passes what it knows.
pub mod context {
    /// The open book's path, title and reading progress in percent are
    /// [`params::PATH`](crate::builtin::params::PATH),
    /// [`params::TITLE`](crate::builtin::params::TITLE) and
    /// [`params::PROGRESS`](crate::builtin::params::PROGRESS).
    pub const CHAPTER: &str = "chapter";
    /// The page as shown, e.g. `12 / 340` (or a percentage while pages are
    /// being counted).
    pub const PAGE: &str = "page";
    /// Where the chapters begin, as thousandths of the book separated by
    /// commas, e.g. `0,112,260`.
    pub const CHAPTERS: &str = "chapters";
}

/// Where and how a status line is drawn (see [`StatusLineSpec`]).
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StatusFrame<'a> {
    pub area: Rect,
    /// The line above the text, else below it.
    pub top: bool,
    /// Text size in pixels, as the user set it for status lines.
    pub size: f32,
    pub color: Color,
    /// What the screen tells about its content (see [`context`]).
    pub context: &'a Params,
    /// The place it is drawn in ([`Place`]), whose items a line of items
    /// shows; `None` where the screen has none.
    pub place: Option<&'a str>,
    /// The font role its text is drawn in: the status lines' own
    /// ([`fonts::STATUS`](crate::builtin::fonts::STATUS)) unless the
    /// screen says another.
    pub font: &'a str,
    /// Levels (the battery) with their percentage beside the symbol, or
    /// the symbol alone.
    pub percent: bool,
}

impl<'a> StatusFrame<'a> {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(area: Rect, top: bool, size: f32, color: Color, context: &'a Params) -> Self {
        Self {
            area,
            top,
            size,
            color,
            context,
            place: None,
            font: crate::builtin::fonts::STATUS.static_str(),
            percent: true,
        }
    }

    /// Levels with their percentage, or the symbol alone.
    pub fn percent(self, percent: bool) -> Self {
        Self { percent, ..self }
    }

    /// Drawn in place `place` ([`Place::id`]).
    pub fn place(self, place: &'a str) -> Self {
        Self {
            place: Some(place),
            ..self
        }
    }

    /// Its text in the font of role `font`.
    pub fn font(self, font: &'a str) -> Self {
        Self { font, ..self }
    }
}

/// Draws a status line into its frame's area (cleared before).
pub type StatusLineFn = Box<dyn Fn(&mut Canvas<'_>, &Env<'_>, &StatusFrame<'_>)>;

/// What a status line shows now, as a number that changes with it (e.g.
/// [`Fnv1a64`](crate::digest::Fnv1a64) of what it shows): the screen
/// redraws the line when it changes (e.g. each minute for a clock).
pub type StatusSignatureFn = Box<dyn Fn(&Env<'_>, &Params) -> u64>;

/// The places in a status line drawn in a frame that do something when
/// tapped (see [`StatusLineSpec::targets`]).
pub type StatusTargetsFn = Box<dyn Fn(&Env<'_>, &StatusFrame<'_>) -> Vec<StatusTarget>>;

/// A place in a status line that does something: a tap on it runs
/// `command`, e.g. an item's [`StatusItemSpec::tap`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StatusTarget {
    pub area: Rect,
    pub command: crate::Command,
}

impl StatusTarget {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(area: Rect, command: crate::Command) -> Self {
        Self { area, command }
    }
}

/// A whole status line contributed by a plugin.
#[non_exhaustive]
pub struct StatusLineSpec {
    pub id: Id,
    /// Translation key of its name in settings, e.g. "Progress bar".
    pub title: Cow<'static, str>,
    pub draw: StatusLineFn,
    /// What it shows; without one it is redrawn when the context
    /// changes (the page, the chapter, …).
    pub signature: Option<StatusSignatureFn>,
    /// Where a tap on it does something; without them a tap does what
    /// the screen does with it (the reader shows its next line).
    pub targets: Option<StatusTargetsFn>,
    /// An id it had before, which lines chosen may still name; lists of
    /// lines show it once, under its own.
    pub also: Option<Id>,
}

impl StatusLineSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, draw: StatusLineFn) -> Self {
        Self {
            id,
            title: title.into(),
            draw,
            signature: None,
            targets: None,
            also: None,
        }
    }

    /// It answers to `id` too, which it had before.
    pub fn also(mut self, id: Id) -> Self {
        self.also = Some(id);
        self
    }

    pub fn signature(mut self, signature: StatusSignatureFn) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Where a tap on it does something, drawn in a frame.
    pub fn targets(mut self, targets: StatusTargetsFn) -> Self {
        self.targets = Some(targets);
        self
    }
}

impl fmt::Debug for StatusLineSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusLineSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// What a status item shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusValue {
    Text(String),
    /// A gauge with a percentage, drawn as a battery.
    Level {
        percent: u8,
        charging: bool,
    },
    /// An icon by name ([`icons`](crate::icons)), e.g. while reading
    /// aloud.
    Icon(Cow<'static, str>),
    /// A short bar filled this far, in thousandths, e.g. how far into a
    /// chapter the page is.
    Bar {
        permille: u16,
    },
    /// An icon by name with a text after it, e.g. Wi-Fi's and the
    /// network's name; shown whole or not at all.
    Labelled {
        icon: Cow<'static, str>,
        text: String,
    },
    /// A kind a later build adds, by its key, with its parameters (as
    /// [`Command::Extended`](crate::Command::Extended)); a build that
    /// doesn't know it leaves the value out.
    Extended {
        key: crate::Id,
        params: crate::Params,
    },
}

/// Computes an item's value; `None` hides it (e.g. no battery).
pub type StatusFn = Box<dyn Fn(&Env<'_>, &Params) -> Option<StatusValue>>;

/// How much an item matters where none says ([`StatusItemSpec::priority`]).
pub const PRIORITY: u8 = 128;

/// A status item contribution.
#[non_exhaustive]
pub struct StatusItemSpec {
    pub id: Id,
    /// Translation key of its name in settings, e.g. "Battery".
    pub title: Cow<'static, str>,
    pub value: StatusFn,
    /// The action a tap on it runs, one of its own plugin's, with the
    /// context its line was drawn with (see [`context`];
    /// [`Command::RunAction`](crate::Command::RunAction)): it
    /// runs as that plugin, whichever screen shows the line, and can be
    /// bound to buttons too. Without it a tap does what the screen does
    /// with the line.
    pub tap: Option<Id>,
    /// How much it matters: where a line has no room for all its items,
    /// those of the lowest priority are left out first ([`PRIORITY`] by
    /// default).
    pub priority: u8,
    /// The least width it takes, in times the text's size, so that the
    /// line does not move when its text gets shorter (a clock's `9:59`
    /// after `10:00`); 0 for its own width.
    pub min_width: f32,
}

impl StatusItemSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, value: StatusFn) -> Self {
        Self {
            id,
            title: title.into(),
            value,
            tap: None,
            priority: PRIORITY,
            min_width: 0.0,
        }
    }

    /// The action a tap on it runs, one of its own plugin's.
    pub fn tap(self, tap: Id) -> Self {
        Self {
            tap: Some(tap),
            ..self
        }
    }

    /// How much it matters (higher stays longer; [`PRIORITY`] by default).
    pub fn priority(self, priority: u8) -> Self {
        Self { priority, ..self }
    }

    /// The least width it takes, in times the text's size.
    pub fn min_width(self, min_width: f32) -> Self {
        Self { min_width, ..self }
    }
}

/// A place where status items show: the settings of the items listed for
/// its left, centre and right (ids separated by commas, see
/// [`item_ids`]). A status line of items drawn there shows these
/// ([`StatusFrame::place`]); the settings' editors may list the places.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Place {
    pub id: Id,
    /// Translation key of its name, e.g. "Below the text".
    pub title: Cow<'static, str>,
    /// The settings of its left, centre and right.
    pub parts: [Id; 3],
    /// The setting of the status line it shows (the first of the ids it
    /// lists), where the place lets the reader choose: without one, or
    /// where it lists none, its items
    /// ([`status_lines::ITEMS`](crate::builtin::status_lines::ITEMS)).
    pub line: Option<Id>,
}

impl Place {
    /// Made of what it needs; the rest as set with its methods.
    pub const fn new(id: Id, title: &'static str, parts: [Id; 3]) -> Self {
        Self {
            id,
            title: Cow::Borrowed(title),
            parts,
            line: None,
        }
    }

    /// The status line it shows is the one setting `line` names first.
    pub const fn with_line(mut self, line: Id) -> Self {
        // Nothing to drop: made without a line (`new`), in a const.
        core::mem::forget(self.line.replace(line));
        self
    }

    /// The status line it shows now: the one its line setting names
    /// first, where there is such a line, else its items.
    pub fn current_line<'e>(&self, env: &Env<'e>) -> &'e str {
        let items = crate::builtin::status_lines::ITEMS.static_str();
        let Some(setting) = &self.line else {
            return items;
        };
        item_ids(env.settings().text(setting.as_str()))
            .find(|id| env.status_items().title(id).is_some())
            .unwrap_or(items)
    }

    /// Draws its line in `frame`, told that it is in this place.
    pub fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, frame: &StatusFrame<'_>) {
        let framed = StatusFrame {
            place: Some(self.id.as_str()),
            ..*frame
        };
        env.status_items()
            .draw_line(self.current_line(env), canvas, env, &framed);
    }

    /// Where a tap on its line, drawn in `frame`, does something.
    pub fn targets(&self, env: &Env<'_>, frame: &StatusFrame<'_>) -> Vec<StatusTarget> {
        let framed = StatusFrame {
            place: Some(self.id.as_str()),
            ..*frame
        };
        env.status_items()
            .line_targets(self.current_line(env), env, &framed)
    }

    /// The items listed for its left, centre and right now.
    pub fn lists<'e>(&self, env: &Env<'e>) -> [&'e str; 3] {
        self.parts
            .each_ref()
            .map(|part| env.settings().text(part.as_str()))
    }

    /// Its items as a line in `area`, with text of `size` in `color`.
    pub fn line<'e>(&self, env: &Env<'e>, area: Rect, size: f32, color: Color) -> StatusLine<'e> {
        StatusLine::new(area, self.lists(env), size, color)
    }

    /// What its line shows now, with `context`: to draw it again when
    /// that changes.
    pub fn signature(&self, env: &Env<'_>, context: &Params) -> u64 {
        let line = self.current_line(env);
        if line == crate::builtin::status_lines::ITEMS.static_str() {
            return signature(env, &self.lists(env), context);
        }
        let mut hash = crate::digest::Fnv1a64::new();
        hash.feed(line.as_bytes());
        hash.feed(
            &env.status_items()
                .line_signature(line, env, context)
                .to_le_bytes(),
        );
        hash.value()
    }
}

/// What the items of `lists` show now, with `context`, as a number that
/// changes with it.
pub fn signature(env: &Env<'_>, lists: &[&str], context: &Params) -> u64 {
    use core::fmt::Write;
    let mut hash = crate::digest::Fnv1a64::new();
    for list in lists {
        for id in item_ids(list) {
            if let Some(value) = env.status_items().value(id, env, context) {
                let _ = write!(hash, "{value:?}|");
            }
        }
        hash.feed(b"/");
    }
    hash.value()
}

impl fmt::Debug for StatusItemSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusItemSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The registered status items.
#[derive(Debug, Default)]
pub struct StatusItems {
    items: Vec<StatusItemSpec>,
    lines: Vec<StatusLineSpec>,
    places: Vec<Place>,
}

impl StatusItems {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, item: StatusItemSpec) {
        self.items.push(item);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.items.shrink_to_fit();
        self.lines.shrink_to_fit();
        self.places.shrink_to_fit();
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_place(&mut self, place: Place) {
        self.places.push(place);
    }

    /// Every place where status items show.
    pub fn places(&self) -> impl Iterator<Item = &Place> {
        self.places.iter()
    }

    /// Place `id`, if there is one.
    pub fn place(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id.as_str() == id)
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_line(&mut self, line: StatusLineSpec) {
        self.lines.push(line);
    }

    /// Every status line: its id and the translation key of its name.
    pub fn lines(&self) -> impl Iterator<Item = (&Id, &str)> {
        self.lines.iter().map(|l| (&l.id, l.title.as_ref()))
    }

    /// The id of status line `id` now: its own where `id` is one it had
    /// before ([`StatusLineSpec::also`]), else `id` as it is.
    pub fn canonical<'a>(&'a self, id: &'a str) -> &'a str {
        self.line(id).map_or(id, |line| line.id.as_str())
    }

    /// Status line `id`, also by an id it had before
    /// ([`StatusLineSpec::also`]).
    fn line(&self, id: &str) -> Option<&StatusLineSpec> {
        self.lines
            .iter()
            .find(|l| l.id.as_str() == id || l.also.as_ref().is_some_and(|a| a.as_str() == id))
    }

    /// Draws status line `id`; `false` if there is none.
    pub fn draw_line(
        &self,
        id: &str,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        frame: &StatusFrame<'_>,
    ) -> bool {
        match self.line(id) {
            Some(line) => {
                (line.draw)(canvas, env, frame);
                true
            }
            None => false,
        }
    }

    /// Where a tap on status line `id`, drawn in `frame`, does something.
    pub fn line_targets(
        &self,
        id: &str,
        env: &Env<'_>,
        frame: &StatusFrame<'_>,
    ) -> Vec<StatusTarget> {
        match self.line(id).and_then(|l| l.targets.as_ref()) {
            Some(targets) => targets(env, frame),
            None => Vec::new(),
        }
    }

    /// What status line `id` shows now (see [`StatusSignatureFn`]).
    pub fn line_signature(&self, id: &str, env: &Env<'_>, context: &Params) -> u64 {
        use core::fmt::Write;
        match self.line(id).and_then(|l| l.signature.as_ref()) {
            Some(signature) => signature(env, context),
            None => {
                let mut hash = crate::digest::Fnv1a64::new();
                let _ = write!(hash, "{context:?}");
                hash.value()
            }
        }
    }

    /// Every item: its id and the translation key of its name.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &str)> {
        self.items.iter().map(|i| (&i.id, i.title.as_ref()))
    }

    /// The translation key of the name of item or status line `id`.
    pub fn title(&self, id: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|i| i.id.as_str() == id)
            .map(|i| i.title.as_ref())
            .or_else(|| self.line(id).map(|l| l.title.as_ref()))
    }

    /// The value of item `id` now.
    pub fn value(&self, id: &str, env: &Env<'_>, context: &Params) -> Option<StatusValue> {
        (self.item(id)?.value)(env, context)
    }

    /// What a tap on item `id`, drawn with `context`, runs: its action
    /// ([`StatusItemSpec::tap`]) with the context, where the action is
    /// its own plugin's.
    pub fn tap(&self, id: &str, context: &Params) -> Option<crate::Command> {
        let item = self.item(id)?;
        let action = item.tap.as_ref()?;
        (action.namespace() == item.id.namespace())
            .then(|| crate::Command::RunAction(action.clone(), context.clone()))
    }

    /// How much item `id` matters ([`StatusItemSpec::priority`]).
    pub fn priority(&self, id: &str) -> u8 {
        self.item(id).map_or(PRIORITY, |i| i.priority)
    }

    pub(crate) fn item(&self, id: &str) -> Option<&StatusItemSpec> {
        self.items.iter().find(|i| i.id.as_str() == id)
    }
}

/// The items of a list as stored in a setting: ids separated by commas.
pub fn item_ids(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|id| !id.is_empty())
}

/// A status line: the items listed (comma-separated ids) for its left,
/// centre and right, drawn in `area` with text of `size` in `color`.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StatusLine<'a> {
    pub area: Rect,
    pub lists: [&'a str; 3],
    pub size: f32,
    pub color: Color,
    /// Levels (the battery) with their percentage beside the symbol, or
    /// the symbol alone.
    pub percent: bool,
}

/// An item as a line places it: where it starts, how wide it is, and
/// what it shows there (a text shortened to its room).
#[derive(Debug)]
struct Laid<'a> {
    id: &'a str,
    value: StatusValue,
    x: f32,
    width: f32,
    percent: bool,
}

/// An item listed in a part of a line: its value now, measured once.
struct Listed<'a> {
    id: &'a str,
    value: StatusValue,
    priority: u8,
    /// Its width with a level's percentage and without it, at least its
    /// least width.
    widths: [f32; 2],
    /// Its least width, in pixels.
    least: f32,
}

impl Listed<'_> {
    fn width(&self, percent: bool) -> f32 {
        self.widths[usize::from(!percent)]
    }

    fn text(&self) -> bool {
        matches!(self.value, StatusValue::Text(_))
    }
}

impl<'a> StatusLine<'a> {
    /// The items of `lists` in `area`, with text of `size` in `color`;
    /// levels with their percentage.
    pub fn new(area: Rect, lists: [&'a str; 3], size: f32, color: Color) -> Self {
        Self {
            area,
            lists,
            size,
            color,
            percent: true,
        }
    }

    /// Levels with their percentage beside the symbol, or the symbol
    /// alone.
    pub fn percent(self, percent: bool) -> Self {
        Self { percent, ..self }
    }

    /// Whether no item is listed.
    pub fn is_empty(&self) -> bool {
        self.lists
            .iter()
            .all(|list| item_ids(list).next().is_none())
    }

    /// Draws the items' current values. The short ones (the page, the
    /// time, the battery) stay whole: the right and centre parts take what
    /// they need, and long texts such as a chapter's title on the left are
    /// shortened with "…" to the rest (keeping a few letters at least).
    /// Where even so not all fit, the battery's percentage is left out,
    /// then the items that matter least ([`StatusItemSpec::priority`]).
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        font: &FontHandle<'_>,
        context: &Params,
    ) {
        // Outside the band drawn now: drawn in its own.
        if !crate::ui::shows(canvas, self.area) {
            return;
        }
        let middle = Middle::of(font, self.area, self.size);
        // Nothing past the line's area, whatever its items.
        let mut canvas = canvas.clipped(self.area);
        for laid in self.layout(env, font, context) {
            draw_value(&mut canvas, env, font, self, &laid, middle);
        }
    }

    /// Where a tap on the line does something: the items with a
    /// [`StatusItemSpec::tap`] as drawn, each with half the gap beside
    /// it, in reading order.
    pub fn targets(
        &self,
        env: &Env<'_>,
        font: &FontHandle<'_>,
        context: &Params,
    ) -> Vec<StatusTarget> {
        let area = self.area;
        let reach = self.size * 0.2;
        let mut laid = self.layout(env, font, context);
        // A few items: sorted by insertion, stable without the 4 KiB of
        // stack `sort_by` takes.
        for i in 1..laid.len() {
            let mut at = i;
            while at > 0 && laid[at - 1].x.total_cmp(&laid[at].x).is_gt() {
                laid.swap(at - 1, at);
                at -= 1;
            }
        }
        laid.iter()
            .filter_map(|item| {
                let command = env.status_items().tap(item.id, context)?;
                let x0 = (item.x - reach).max(f32::from(area.x));
                let x1 =
                    (item.x + item.width + reach).min(f32::from(area.x) + f32::from(area.width));
                let rect = Rect::new(x0 as u16, area.y, (x1 - x0).max(0.0) as u16, area.height);
                Some(StatusTarget::new(rect, command))
            })
            .collect()
    }

    /// Where each item goes, what does not fit left out.
    fn layout(&self, env: &Env<'_>, font: &FontHandle<'_>, context: &Params) -> Vec<Laid<'a>> {
        let (area, size) = (self.area, self.size);
        let mut parts: [Vec<Listed<'a>>; 3] = self.lists.map(|list| {
            item_ids(list)
                .filter_map(|id| listed(env, font, size, id, context))
                .collect()
        });
        let percent = fit(size, f32::from(area.width), &mut parts, self.percent);
        let [left, center, right] = parts;
        let measure = |item: &Listed<'_>| item.width(percent);
        // A gap as wide as the text is high; narrower, down to 40 %, where
        // the items would not fit whole with it (wide margins, large text).
        let all = [&left, &center, &right];
        let inked: f32 = all.iter().flat_map(|p| p.iter()).map(measure).sum();
        let gaps = all.iter().map(|p| p.len()).sum::<usize>().saturating_sub(1);
        let gap = if gaps == 0 {
            size
        } else {
            ((f32::from(area.width) - inked) / gaps as f32).clamp(size * 0.4, size)
        };
        let width = |items: &[Listed<'_>]| -> f32 {
            items.iter().map(measure).sum::<f32>() + gap * items.len().saturating_sub(1) as f32
        };
        let x0 = f32::from(area.x);
        let x1 = x0 + f32::from(area.width);
        let line = f32::from(area.width);
        // The left keeps a few letters, the centre all it needs; the
        // right gets the rest.
        let left_room = if left.is_empty() {
            0.0
        } else {
            width(&left).min(size * 3.0)
        };
        let center_need = if center.is_empty() {
            0.0
        } else {
            width(&center) + gap
        };
        let beside = left_room + if left.is_empty() { 0.0 } else { gap } + center_need;
        let right_room = (line - beside).max(line / 2.0);
        let right_x = x1 - width(&right).min(right_room);
        let center_room = (right_x - gap - x0 - left_room).max(0.0);
        let center_w = width(&center).min(center_room);
        // Centred, unless that runs into the right part.
        let center_x = (x0 + (line - center_w) / 2.0)
            .min(right_x - gap - center_w)
            .max(x0 + left_room);
        let left_end = if center.is_empty() { right_x } else { center_x } - gap;
        let mut laid = Vec::new();
        let mut place = |items: Vec<Listed<'a>>, x: f32, end: f32| {
            place(&mut laid, font, (size, gap, percent), items, (x, end));
        };
        place(center, center_x, right_x - gap);
        place(right, right_x, x1);
        place(left, x0, left_end);
        laid
    }
}

/// Item `id` with its value now (`None` where it shows nothing), measured
/// at `size`.
fn listed<'a>(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    size: f32,
    id: &'a str,
    context: &Params,
) -> Option<Listed<'a>> {
    let item = env.status_items().item(id)?;
    let value = (item.value)(env, context)?;
    let least = item.min_width * size;
    let width = |percent| value_width(env, font, size, &value, percent).max(least);
    Some(Listed {
        id,
        widths: [width(true), width(false)],
        value,
        priority: item.priority,
        least,
    })
}

/// Leaves out of `parts` what does not fit `room` whole, and says whether
/// levels show their percentage (where `percent` wants it): the
/// percentage goes first, then the items of the lowest priority, of
/// equal ones the centre's, then the left's, then the right's, each
/// part's from its end; after each, the percentage is tried again. Texts
/// count with a few letters, as they are shortened to their room.
fn fit(size: f32, room: f32, parts: &mut [Vec<Listed<'_>>; 3], percent: bool) -> bool {
    loop {
        let levels = parts
            .iter()
            .flatten()
            .any(|item| matches!(item.value, StatusValue::Level { .. }));
        let tries: &[bool] = if percent && levels {
            &[true, false]
        } else {
            &[false]
        };
        for &with in tries {
            let count: usize = parts.iter().map(Vec::len).sum();
            let mut need = size * 0.4 * count.saturating_sub(1) as f32;
            for item in parts.iter().flatten() {
                let width = item.width(with);
                need += if item.text() {
                    width.min(size * 3.0)
                } else {
                    width
                };
            }
            // A pixel of slack for rounding, as when the parts are placed.
            if need <= room + 1.0 {
                return with && percent;
            }
        }
        let mut least: Option<(u8, usize, usize)> = None;
        for (rank, at) in [1, 0, 2].into_iter().enumerate() {
            for (index, item) in parts[at].iter().enumerate().rev() {
                if least.is_none_or(|(p, r, _)| (item.priority, rank) < (p, r)) {
                    least = Some((item.priority, rank, index));
                }
            }
        }
        let Some((_, rank, index)) = least else {
            return false;
        };
        parts[[1, 0, 2][rank]].remove(index);
    }
}

fn value_width(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    size: f32,
    value: &StatusValue,
    with_percent: bool,
) -> f32 {
    match value {
        StatusValue::Icon(name) => crate::ui::icon_width(env, name, size),
        StatusValue::Text(text) => font.measure(size, text),
        StatusValue::Level { percent, .. } if with_percent => {
            battery_width(size) + size * 0.3 + font.measure(size, &level_text(*percent))
        }
        StatusValue::Level { .. } => battery_width(size),
        StatusValue::Bar { .. } => bar_width(size),
        StatusValue::Labelled { icon, text } => {
            crate::ui::icon_width(env, icon, size) + label_gap(size) + font.measure(size, text)
        }
        // A value of a later build: left out.
        StatusValue::Extended { .. } => 0.0,
    }
}

/// A bar's length: about as long as a short word.
fn bar_width(size: f32) -> f32 {
    size * 4.0
}

/// Between a labelled item's icon and its text.
fn label_gap(size: f32) -> f32 {
    size * 0.25
}

fn level_text(percent: u8) -> String {
    alloc::format!("{percent} %")
}

fn battery_width(size: f32) -> f32 {
    size * 1.3
}

/// Places the `items` of a part from `x` to `end`, with `gap`s between
/// them: what is not text whole or not at all, a gauge without its
/// percentage where only the gauge fits, the longest text shortened
/// where they do not all fit.
fn place<'a>(
    out: &mut Vec<Laid<'a>>,
    font: &FontHandle<'_>,
    (size, gap, with_percent): (f32, f32, bool),
    items: Vec<Listed<'a>>,
    (mut x, end): (f32, f32),
) {
    let widths: Vec<f32> = items.iter().map(|item| item.width(with_percent)).collect();
    let texts: Vec<bool> = items.iter().map(Listed::text).collect();
    let shrunk = shrink(&widths, &texts, gap, end - x);
    let budget = |i: usize| match shrunk {
        Some((at, width)) if at == i => width,
        _ => f32::MAX,
    };
    for (i, item) in items.into_iter().enumerate() {
        if x >= end {
            break;
        }
        // A pixel of slack for rounding, as for texts.
        let fits = |width: f32| x + width <= end + 1.0;
        let percent = with_percent && fits(widths[i]);
        let (value, width) = match item.value {
            StatusValue::Text(text) => {
                // A pixel of slack for rounding: the parts were measured to fit.
                let room = (end - x + 1.0).min(budget(i));
                let text = crate::ui::fit(font, size, &text, room);
                let width = font.measure(size, &text).max(item.least.min(room));
                (StatusValue::Text(text), width)
            }
            value => {
                let width = item.widths[usize::from(!percent)];
                if !fits(width) {
                    break;
                }
                (value, width)
            }
        };
        out.push(Laid {
            id: item.id,
            value,
            x,
            width,
            percent,
        });
        x += width + gap;
    }
}

/// The one line a status line's items are centred on: the middle of its
/// figures' ink (the digits of the time, the page, the percentage), in
/// the middle of the line's band, top and bottom alike. Text stands on
/// `baseline`; shapes and icons are centred on `twice` / 2 (twice the y,
/// so that a centre between two pixels is said exactly).
#[derive(Debug, Clone, Copy)]
struct Middle {
    baseline: i32,
    twice: i32,
    /// The figures' height, in pixels.
    figures: i32,
}

/// What a status line's figures are measured by.
const FIGURES: &str = "0123456789";

impl Middle {
    fn of(font: &FontHandle<'_>, area: Rect, size: f32) -> Self {
        let (top, bottom) = font.ink(size, FIGURES).unwrap_or_else(|| {
            let metrics = font.line_metrics(size);
            (-(metrics.ascent as i32), -(metrics.descent as i32))
        });
        let room = i32::from(area.height) - (bottom - top);
        // An odd pixel goes below, where the eye expects it.
        let baseline = i32::from(area.y) + room.div_euclid(2) - top;
        Middle {
            baseline,
            twice: 2 * baseline + top + bottom,
            figures: bottom - top,
        }
    }

    /// `height` made as long as it can be centred on the line to the
    /// pixel: as odd or as even as the figures' height (a pixel less
    /// where it is not), and the top of a shape that tall on it.
    fn fit(&self, height: i32) -> (i32, i32) {
        let height = if (height - self.figures) % 2 == 0 {
            height
        } else {
            height - 1
        }
        .max(1);
        (height, (self.twice - height).div_euclid(2))
    }
}

/// Draws an item where the line placed it, on the line's middle.
fn draw_value(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    line: &StatusLine<'_>,
    laid: &Laid<'_>,
    middle: Middle,
) {
    let (size, color) = (line.size, line.color);
    let metrics = font.line_metrics(size);
    let (x, baseline) = (laid.x, middle.baseline);
    match &laid.value {
        // A value of a later build: left out.
        StatusValue::Extended { .. } => {}
        StatusValue::Text(text) => {
            font.draw(canvas, size, x, baseline, text, color);
        }
        StatusValue::Icon(name) => {
            crate::ui::draw_icon_centred(canvas, env, name, size, x, middle.twice, color);
        }
        StatusValue::Level { percent, charging } => {
            let (height, top) = middle.fit((metrics.ascent * 0.75) as i32);
            let battery = Rect::new(
                x as u16,
                top.max(0) as u16,
                battery_width(size) as u16,
                height as u16,
            );
            draw_battery(canvas, battery, size, *percent, *charging, color);
            if laid.percent {
                let at = x + battery_width(size) + size * 0.3;
                font.draw(canvas, size, at, baseline, &level_text(*percent), color);
            }
        }
        StatusValue::Bar { permille } => {
            // As thick as the line of the book's progress bar.
            let (thickness, top) = middle.fit((size * 0.3).max(3.0) as i32);
            let thickness = thickness as u32;
            let stroke = (size / 12.0).max(1.0) as u32;
            let width = bar_width(size);
            canvas.stroke_rect(x as i32, top, width as u32, thickness, stroke, color);
            let filled = width * f32::from((*permille).min(1000)) / 1000.0;
            canvas.fill_rect(x as i32, top, filled as u32, thickness, color);
        }
        StatusValue::Labelled { icon, text } => {
            let icon_width =
                crate::ui::draw_icon_centred(canvas, env, icon, size, x, middle.twice, color);
            let at = x + icon_width + label_gap(size);
            font.draw(canvas, size, at, baseline, text, color);
        }
    }
}

/// When values of `widths` (with `gap`s between them) do not fit `room`:
/// the longest text (`texts`) and the width it may take, so short ones
/// (the page, the time) stay whole. A pixel of slack for rounding: parts
/// are placed by their own width.
fn shrink(widths: &[f32], texts: &[bool], gap: f32, room: f32) -> Option<(usize, f32)> {
    let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    if total <= room + 1.0 {
        return None;
    }
    let longest = (0..widths.len())
        .filter(|&i| texts[i])
        .max_by(|&a, &b| widths[a].total_cmp(&widths[b]))?;
    Some((longest, (widths[longest] - (total - room)).max(0.0)))
}

/// A battery filled to `percent`, in the style of the UI's shapes: a rounded
/// body, a small rounded nub, the level as a rounded bar inside with a
/// gap as wide as the line all round, and a bolt when charging, in ink
/// with a rim of paper, so that it reads on the level, beside it
/// and across its edge alike. Crisp at every size: whole pixels only,
/// nothing blended.
fn draw_battery(
    canvas: &mut Canvas<'_>,
    area: Rect,
    size: f32,
    percent: u8,
    charging: bool,
    color: Color,
) {
    let (x, y, height) = (i32::from(area.x), i32::from(area.y), u32::from(area.height));
    let line = ((size / 10.0) as u32).max(1);
    let nub = ((size / 8.0) as u32).max(2);
    let width = u32::from(area.width).saturating_sub(nub);
    let radius = (height / 4).max(1);
    canvas.stroke_round_rect(x, y, width, height, line, radius, color);
    // The nub, half as tall as the body and centred on it to the pixel,
    // rounded on its outer side (its inner side lies on the body's line).
    let mut nub_height = height / 2;
    if (height - nub_height) % 2 == 1 {
        nub_height += 1;
    }
    let nub_radius = line.min(nub_height / 2).max(1);
    canvas.fill_round_rect(
        x + width as i32 - nub_radius as i32,
        y + ((height - nub_height) / 2) as i32,
        nub + nub_radius,
        nub_height,
        nub_radius,
        color,
    );
    // The level: a bar inside, as far from the line as the line is wide.
    let inset = 2 * line;
    let bar_height = height.saturating_sub(2 * inset);
    let full = width.saturating_sub(2 * inset);
    let fill = full * u32::from(percent.min(100)) / 100;
    let bar_radius = radius.saturating_sub(inset).max(1).min(bar_height / 2);
    let (bar_x, bar_y) = (x + inset as i32, y + inset as i32);
    if fill > 0 && bar_height > 0 {
        canvas.fill_round_rect(bar_x, bar_y, fill, bar_height, bar_radius, color);
    }
    if !charging {
        return;
    }
    // The bolt in the middle of the inside: in ink, with a rim of paper
    // a pixel wide inside the body.
    let mark_height = height.saturating_sub(2 * line + 2).max(3);
    let mark_width = (mark_height * 2 / 3).max(4);
    let left = x + (width as i32 - mark_width as i32) / 2;
    let top = y + (height as i32 - mark_height as i32) / 2;
    let in_mark = |column: i32, row: i32| {
        if column < 0 || row < 0 || column >= mark_width as i32 || row >= mark_height as i32 {
            return false;
        }
        in_bolt(column as u32, row as u32, mark_width, mark_height, line + 1)
    };
    let (inside_left, inside_right) = (x + line as i32, x + (width - line) as i32);
    let (inside_top, inside_bottom) = (y + line as i32, y + (height - line) as i32);
    for row in -1..=mark_height as i32 {
        for column in -1..=mark_width as i32 {
            let (px, py) = (left + column, top + row);
            let ink = if in_mark(column, row) {
                color
            } else if (-1..=1).any(|dy| (-1..=1).any(|dx| in_mark(column + dx, row + dy))) {
                Color::WHITE
            } else {
                continue;
            };
            if px >= inside_left && px < inside_right && py >= inside_top && py < inside_bottom {
                canvas.fill_rect(px, py, 1, 1, ink);
            }
        }
    }
}

/// Whether the pixel at `column` and `row` of a box `width` by `height`
/// is a lightning bolt's, drawn as pixels are at small sizes: a stroke
/// `thickness` wide from the top right down to the middle's left, a bar
/// across the middle, and a stroke from the middle's right down to the
/// bottom left.
fn in_bolt(column: u32, row: u32, width: u32, height: u32, thickness: u32) -> bool {
    let thickness = thickness.min(width / 2).max(1);
    let middle = height / 2;
    let run = width - thickness;
    let left = if row < middle {
        // Rounded to the nearest pixel, as the steps fall.
        (run * (middle - row) + middle / 2) / middle.max(1)
    } else if row == middle {
        return column < width;
    } else {
        let rest = height - 1 - middle;
        (run * (height - 1 - row) + rest / 2) / rest.max(1)
    };
    column >= left && column < left + thickness
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lines_too_long_give_way() {
        // Battery and time placed by their own width: rounding noise is
        // no reason to cut the time.
        let widths = [60.0, 70.1];
        let room = 70.1 + 60.0 + 20.0 - 0.000_01;
        assert_eq!(shrink(&widths, &[false, true], 20.0, room), None);
        // Too long: the longest text gives way, the page stays whole.
        let widths = [300.0, 80.0];
        assert_eq!(
            shrink(&widths, &[true, true], 20.0, 250.0),
            Some((0, 150.0))
        );
        // Nothing to shorten without text.
        assert_eq!(shrink(&[100.0], &[false], 0.0, 50.0), None);
    }
}
