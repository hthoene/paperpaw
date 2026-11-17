//! The home screen's own widgets on the flat native boundary: the host makes an
//! instance when a screen shows one, asks it for its height and where it sits,
//! and calls the plugin to draw it. Nothing of a callback is kept; an instance
//! is the widget's id with its placement. The widgets are the date, the clock,
//! the continue card and the recent shelf; the status line follows
//! (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        commands::CommandSender,
        fonts::Font,
        image::{ImageId, ImageStyle},
        reply::Reply,
        ui::{Instance, Open, Param, Ui, WidgetInputKind, WidgetTarget, stages::WidgetCall},
    },
};
use paperpaw_ui_kit::{
    Paint, icons,
    mem::buffer,
    paint, status_line,
    text::copy,
    widget::{Cx, Kept, cx::widget_font},
};

use crate::declarations::{CARD, CLOCK, DATE, SHELF, STATUS_WIDGET};
use crate::native_recent::{self, Ask, Known, Listing, Pictures};
use crate::{native_card, native_clock, native_date, native_shelf, native_status};

/// The most bytes of a stage reply (sizes and no targets).
const REPLY: usize = 256;

/// A widget of ours as it shows somewhere: what draws it, measures it and
/// answers its taps, whether the host made an instance of it for a screen of
/// another plugin or this plugin's own screen shows it (the host never calls
/// the plugin back for its own widget: it draws it itself).
#[derive(Debug)]
pub struct Mine {
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// A cover it drew was still being made: the next tick asks the screen to
    /// draw it again.
    waiting: Cell<bool>,
    /// The books of the card or the shelf whose files the host found gone.
    known: Known,
    /// Where the screen last placed it, for a tap on one of its items.
    placed: Cell<Option<Rect>>,
    /// The look its settings make, read by the first draw, measure or tap and
    /// kept for the later ones (a frame draws it once per band): the interface's
    /// style and its own are some thirty settings. Let go where a setting or the
    /// registry changes ([`Widgets::forget_looks`]).
    look: Looks,
    /// What it composed for the bands of the frame under way: the first band
    /// measures, wraps and paints the whole widget, the later ones draw the
    /// same ([`Mine::draw`]).
    banded: RefCell<Vec<Banded>>,
    /// The covers' places were drawn without them, the covers to follow.
    held: Cell<bool>,
    /// What the card or the shelf composed at the last frame it drew whole,
    /// kept across returns for as long as what it composes from is the same
    /// (`content_key`): the measuring and wrapping of its lines is most of what
    /// a return spends on text (#294). Let go with the look, and where memory
    /// is short.
    memo: RefCell<Vec<Memo>>,
}

/// The most a kept composition may take, in bytes (the paints and the covers'
/// places; the lines' own strings are short): the card's and the shelf's
/// together stay within 3 KB.
const MEMO_BYTES: usize = 1536;

/// A composition kept for the next frame and the content it was made from.
#[derive(Debug)]
struct Memo {
    key: u64,
    kept: Banded,
}

/// What a widget composed for a frame's first band, for the next ones.
#[derive(Debug)]
struct Banded {
    /// What the composition was made from (`Mine::content_key`), where it may
    /// be kept for a later frame.
    key: Option<u64>,
    /// A cover it drew was still being made.
    busy: bool,
    area: Rect,
    epoch: u32,
    /// The rows of the last band drawn with it.
    bottom: i32,
    out: Vec<Paint>,
    placed: Vec<(ImageId, Rect)>,
}

/// The look a widget kept: none or one, in a vector so that keeping it is
/// fallible (a look not kept is read again at the next draw) and an instance
/// that never drew holds nothing. With it, for the status line only, what it
/// asked the host about its settings and registry, which only a setting or
/// registry change alters: the first frame of a return made three service
/// calls for it (#294).
#[derive(Debug, Default)]
pub struct Looks {
    kept: RefCell<Vec<Kept>>,
    status: RefCell<Vec<StatusKept>>,
}

/// What the status line kept of its host answers.
#[derive(Debug, Default)]
pub struct StatusKept {
    /// Whether the reader chose the widget's own paper or ink.
    own: Option<bool>,
    /// The whole line the `line` setting's ids (the key) show now.
    line: Option<(String, String)>,
    /// What the registry says of the items the lists name.
    pub items: status_line::Memo,
}

impl Looks {
    /// The look kept, if any, handed over: the draw that takes it keeps it again.
    pub fn take(&self) -> Option<Kept> {
        self.kept.borrow_mut().pop()
    }

    /// Keeps `kept` in place of any other, if there is room.
    pub fn keep(&self, kept: Kept) {
        let mut held = self.kept.borrow_mut();
        held.clear();
        if held.try_reserve_exact(1).is_ok() {
            held.push(kept);
        }
    }

    /// Lets go of what is kept.
    pub fn forget(&self) {
        self.kept.borrow_mut().clear();
        self.status.borrow_mut().clear();
    }

    /// Runs `f` on what the status line kept (made empty at first).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn with_status<R>(&self, f: impl FnOnce(&mut StatusKept) -> R) -> Result<R, Error> {
        let mut held = self.status.borrow_mut();
        if held.is_empty() {
            held.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
            held.push(StatusKept::default());
        }
        let kept = held.first_mut().ok_or(Error::NoMemory)?;
        Ok(f(kept))
    }

    /// Whether the widget has a look of its own, asked of `ask` once for as
    /// long as nothing is forgotten.
    ///
    /// # Errors
    /// What `ask` returns.
    pub fn own(&self, ask: impl FnOnce() -> Result<bool, Error>) -> Result<bool, Error> {
        if let Some(own) = self.with_status(|kept| kept.own)? {
            return Ok(own);
        }
        let own = ask()?;
        self.with_status(|kept| kept.own = Some(own))?;
        Ok(own)
    }

    /// The line shown for the ids `listed`: the one kept for the same ids, else
    /// what `ask` finds, kept (`fresh`: asked in any case, to learn of a line
    /// that came or went without an event).
    ///
    /// # Errors
    /// What `ask` returns.
    pub fn line(
        &self,
        listed: &str,
        fresh: bool,
        ask: impl FnOnce() -> Result<String, Error>,
    ) -> Result<String, Error> {
        if !fresh {
            let found = self.with_status(|kept| match &kept.line {
                Some((ids, line)) if ids == listed => Some(copy(line)),
                _ => None,
            })?;
            if let Some(line) = found {
                return line;
            }
        }
        let line = ask()?;
        let kept = (copy(listed), copy(&line));
        self.with_status(|status| {
            status.line = match kept {
                (Ok(ids), Ok(line)) => Some((ids, line)),
                _ => None,
            };
        })?;
        Ok(line)
    }
}

/// The widget an instance is.
#[derive(Debug)]
struct Item {
    instance: Instance,
    /// What it showed when the tick last looked; `None` until the first tick,
    /// which only records it (the draw does not set it).
    shown: Option<u64>,
    mine: Mine,
}

/// The instances the screens made of the widgets.
#[derive(Debug, Default)]
pub struct Widgets {
    items: Vec<Item>,
    /// What the instances' card and shelf share of the library.
    listing: Listing,
}

/// The token of the widget the host calls `widget` (`home.date`, or placed
/// `home.date@2`), if it is one of ours here.
fn token_of(widget: &str) -> Option<u32> {
    match paperpaw_ui_kit::widget::settings::base(widget) {
        "home.date" => Some(DATE),
        "home.clock" => Some(CLOCK),
        "home.continue" => Some(CARD),
        "home.recent" => Some(SHELF),
        "home.status" => Some(STATUS_WIDGET),
        _ => None,
    }
}

impl Mine {
    /// The widget `widget` (`home.date`, or placed `home.date@2`) as ours;
    /// `None` for one that is another plugin's.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(widget: &str) -> Result<Option<Self>, Error> {
        let Some(token) = token_of(widget) else {
            return Ok(None);
        };
        Ok(Some(Self {
            token,
            widget: copy(widget)?,
            waiting: Cell::new(false),
            known: Known::default(),
            placed: Cell::new(None),
            look: Looks::default(),
            banded: RefCell::new(Vec::new()),
            memo: RefCell::new(Vec::new()),
            held: Cell::new(false),
        }))
    }

    fn ask<'a>(&'a self, listing: &'a Listing) -> Ask<'a> {
        Ask {
            known: &self.known,
            listing,
        }
    }

    /// Lets go of the look it kept: the next draw reads it again.
    pub fn forget_look(&self) {
        self.look.forget();
        self.banded.borrow_mut().clear();
        self.memo.borrow_mut().clear();
    }

    /// Whether it shows covers: the card and the shelf.
    pub fn has_covers(&self) -> bool {
        matches!(self.token, CARD | SHELF)
    }

    /// Whether it waits for a library cover.
    pub fn waits_for_cover(&self) -> bool {
        self.waiting.get()
    }

    pub fn covers_ready(&self) -> bool {
        self.waiting.replace(false)
    }

    /// Where the screen placed it, for a tap on one of its items.
    pub fn place(&self, rect: Rect) {
        self.placed.set(Some(rect));
    }

    /// Its height at `width` pixels, and the least height it can do with.
    ///
    /// # Errors
    /// Font and host refusals, [`Error::NoMemory`].
    pub fn measure(
        &self,
        host: &Host<'_>,
        listing: &Listing,
        width: u32,
    ) -> Result<(u16, u16), Error> {
        let mut cx = Cx::open_kept_with(host, &self.widget, self.look.take(), listing.inputs())?;
        let width = u16::try_from(width).unwrap_or(u16::MAX);
        let size = match self.token {
            DATE => same(native_date::height(&mut cx, width)?),
            CARD => (native_card::height(&cx), native_card::height(&cx) / 2),
            SHELF => native_shelf::height(&cx, self.ask(listing), width)?,
            STATUS_WIDGET => same(native_status::height(&cx)?),
            _ => same(native_clock::height(&mut cx, width)?),
        };
        self.look.keep(cx.keep());
        Ok(size)
    }

    /// What a tap can reach in `rect`: the card's and the shelf's books, the
    /// status line's items (none for the date and the clock).
    ///
    /// # Errors
    /// Font and host refusals, [`Error::NoMemory`].
    pub fn targets(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        listing: &Listing,
        rect: Rect,
    ) -> Result<Vec<WidgetTarget>, Error> {
        let mut found: Vec<WidgetTarget> = Vec::new();
        match self.token {
            STATUS_WIDGET => {
                let cx =
                    Cx::open_kept_with(host, &self.widget, self.look.take(), listing.inputs())?;
                let inner = cx.shape.look(rect).area;
                for (id, at) in native_status::targets(&cx, ui, &self.look, inner)? {
                    let target = WidgetTarget::new(id, paint::frame_rect(at));
                    paperpaw_ui_kit::mem::push(&mut found, target)?;
                }
                self.look.keep(cx.keep());
            }
            CARD => paperpaw_ui_kit::mem::push(
                &mut found,
                WidgetTarget::new(native_card::TARGET, paint::frame_rect(rect)),
            )?,
            SHELF => {
                let mut cx =
                    Cx::open_kept_with(host, &self.widget, self.look.take(), listing.inputs())?;
                for (id, at) in native_shelf::targets(&mut cx, self.ask(listing), rect)? {
                    let target = WidgetTarget::new(id, paint::frame_rect(at));
                    paperpaw_ui_kit::mem::push(&mut found, target)?;
                }
                self.look.keep(cx.keep());
            }
            _ => {}
        }
        Ok(found)
    }

    /// Draws it into `area` of `frame` (a widget's draw, or the screen's that
    /// shows it).
    ///
    /// # Errors
    /// Font and host refusals, [`Error::NoMemory`].
    pub fn draw(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        listing: &Listing,
        frame: &mut DrawFrame<'_>,
        area: Rect,
    ) -> Result<(), Error> {
        if self.token == STATUS_WIDGET {
            return native_status::draw_widget(
                host,
                ui,
                frame,
                (&self.widget, &self.look, listing.inputs()),
                area,
            );
        }
        self.known.begin_draw(frame.clip_top, listing);
        listing.drawing(true);
        let drawn = self.draw_own(host, listing, frame, area);
        listing.drawing(false);
        if matches!(drawn, Err(Error::NoMemory)) {
            // The room is wanted: what was kept for later goes.
            self.memo.borrow_mut().clear();
            self.banded.borrow_mut().clear();
        }
        drawn
    }

    /// The rows below the widget's area, where the last band that draws it ends.
    fn end_of(frame: &DrawFrame<'_>, area: Rect) -> i32 {
        frame
            .rect
            .y
            .saturating_add(i32::from(area.y))
            .saturating_add(i32::from(area.height))
    }

    /// What the first band of the frame composed, for the band after it: the
    /// first band measures and wraps the whole widget and every band after
    /// it did it again (the X4 draws a frame in two to five bands). Only a band
    /// that starts where the last one stopped, with nothing but draws between
    /// them, takes it; the last band of the widget lets it go.
    fn take_banded(&self, listing: &Listing, frame: &DrawFrame<'_>, area: Rect) -> Option<Banded> {
        let kept = self.banded.borrow_mut().pop()?;
        (kept.area == area && kept.epoch == listing.epoch() && kept.bottom == frame.clip_top)
            .then_some(kept)
    }

    fn keep_banded(&self, frame: &DrawFrame<'_>, area: Rect, mut kept: Banded) {
        if frame.clip_bottom >= Self::end_of(frame, area) {
            // The last band of the widget: the composition stays for a frame
            // to come where it is small and made from something that can be
            // told again.
            self.keep_memo(kept);
            return;
        }
        kept.bottom = frame.clip_bottom;
        let mut held = self.banded.borrow_mut();
        if held.try_reserve_exact(1).is_ok() {
            held.push(kept);
        }
    }

    /// What the card or the shelf composes from, as one number (`None`: the
    /// widget is not kept, or the content cannot be told).
    fn content_key(&self, cx: &Cx<'_>, listing: &Listing, area: Rect) -> Option<u64> {
        let ask = self.ask(listing);
        match self.token {
            CARD => native_card::key(cx, ask, area),
            SHELF => native_shelf::key(cx, ask, area),
            _ => return None,
        }
        .ok()
        .flatten()
    }

    fn take_memo(&self, key: u64, area: Rect) -> Option<Banded> {
        let memo = self.memo.borrow_mut().pop()?;
        (memo.key == key && memo.kept.area == area).then_some(memo.kept)
    }

    fn keep_memo(&self, kept: Banded) {
        let Some(key) = kept.key else {
            return;
        };
        let bytes = kept
            .out
            .len()
            .saturating_mul(core::mem::size_of::<Paint>())
            .saturating_add(
                kept.placed
                    .len()
                    .saturating_mul(core::mem::size_of::<(ImageId, Rect)>()),
            );
        let mut held = self.memo.borrow_mut();
        held.clear();
        // Kept only where there is room: a refusal costs the next return its
        // composition, nothing else.
        if bytes <= MEMO_BYTES && held.try_reserve_exact(1).is_ok() {
            held.push(Memo { key, kept });
        }
    }

    fn draw_own(
        &self,
        host: &Host<'_>,
        listing: &Listing,
        frame: &mut DrawFrame<'_>,
        area: Rect,
    ) -> Result<(), Error> {
        let icons = if self.token == CARD {
            icons::font(host)?
        } else {
            None
        };
        if let Some(kept) = self.take_banded(listing, frame, area) {
            let font = widget_font(host, &self.widget)?;
            let drawn = self.replay(listing, frame, (&font, icons.as_ref()), &kept);
            self.keep_banded(frame, area, kept);
            return drawn;
        }
        let mut cx = Cx::open_kept_with(host, &self.widget, self.look.take(), listing.inputs())?;
        // The first band of a frame whose content is what was composed last
        // time: its lines are drawn again, not measured and wrapped again.
        if let Some(key) = self.content_key(&cx, listing, area)
            && let Some(mut kept) = self.take_memo(key, area)
        {
            kept.epoch = listing.epoch();
            kept.bottom = frame.clip_top;
            self.waiting.set(kept.busy);
            let drawn = self.replay(listing, frame, (&cx.font, icons.as_ref()), &kept);
            self.keep_banded(frame, area, kept);
            self.look.keep(cx.keep());
            return drawn;
        }
        let mut out = Vec::new();
        let mut pictures = Pictures::default();
        match self.token {
            DATE => native_date::draw(&mut cx, &mut out, area)?,
            CARD => native_card::draw(
                &mut cx,
                &mut out,
                icons.as_ref(),
                (&mut pictures, self.ask(listing)),
                area,
            )?,
            SHELF => {
                native_shelf::draw(&mut cx, &mut out, (&mut pictures, self.ask(listing)), area)?
            }
            _ => native_clock::draw(&mut cx, &mut out, area)?,
        }
        self.waiting.set(pictures.busy);
        let kept = Banded {
            // Told after the composition: what it asked of the library is known.
            key: self.content_key(&cx, listing, area),
            busy: pictures.busy,
            area,
            epoch: listing.epoch(),
            bottom: frame.clip_top,
            out,
            placed: pictures.placed,
        };
        let drawn = self.replay(listing, frame, (&cx.font, icons.as_ref()), &kept);
        self.keep_banded(frame, area, kept);
        self.look.keep(cx.keep());
        drawn
    }

    /// Replays what was composed in the band of `frame`: the paint, then the
    /// covers, which go where nothing else is drawn. One the host no longer has
    /// (it ended the id for room) or cannot draw is left out: the next draw asks
    /// for it again.
    fn replay(
        &self,
        listing: &Listing,
        frame: &mut DrawFrame<'_>,
        (font, icons): (&Font<'_>, Option<&Font<'_>>),
        kept: &Banded,
    ) -> Result<(), Error> {
        paint::replay_icons(frame, font, icons, &kept.out)?;
        // The covers come in the frame after this one (`COVERS_IN_FOLLOWUP`):
        // here each has its place drawn as a frame, so that nothing jumps.
        let follow = frame.id & 1 == 1;
        let later = listing.later() && !follow;
        if follow {
            // The frame after the page's: the covers come now.
            listing.covers_later(false);
            if self.held.replace(false) {
                self.waiting.set(kept.busy);
            }
        }
        // The rows of this band, as the paint's: a cover wholly outside it
        // changes no pixel and costs a host call.
        let (top, bottom) = (
            frame.clip_top.saturating_sub(frame.rect.y),
            frame.clip_bottom.saturating_sub(frame.rect.y),
        );
        for (image, at) in &kept.placed {
            if top < bottom
                && (i32::from(at.y) + i32::from(at.height) <= top || i32::from(at.y) >= bottom)
            {
                continue;
            }
            if later {
                let place = Paint::Focus(*at, 0, Some(2), Color::BLACK);
                paint::replay(frame, font, [place].iter())?;
                // Asked again at the next minute if the frame after never came.
                self.held.set(true);
                self.waiting.set(true);
                continue;
            }
            let drawn = frame.draw_canvas()?.draw_image(
                *image,
                paint::frame_rect(*at),
                ImageStyle::new().small(),
            );
            if drawn.is_err() {
                // The host ended the id: the next tick draws it, asked for anew.
                listing.forget_all_covers();
                self.waiting.set(true);
            }
        }
        Ok(())
    }

    /// A tap on a target of the card or the shelf opens the book it shows (the
    /// card with no book: the library). Anything else is not handled.
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn tap(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        listing: &Listing,
        target: u32,
        kind: WidgetInputKind,
    ) -> Result<bool, Error> {
        if kind != WidgetInputKind::Tap || target == 0 {
            return Ok(false);
        }
        if self.token == STATUS_WIDGET {
            return status_tap(host, ui, self, target);
        }
        let book = match self.token {
            CARD => native_recent::read(host, listing, 0, 1)?.into_iter().next(),
            SHELF => {
                let cx = Cx::open(host, &self.widget)?;
                let at = usize::try_from(target - 1).map_err(|_| Error::Invalid)?;
                native_shelf::books(&cx, self.ask(listing))?
                    .into_iter()
                    .nth(at)
            }
            _ => return Ok(false),
        };
        let mut scratch = [0; 256];
        let mut commands = CommandSender::new(host, &mut scratch);
        match book {
            Some(book) => open_book(host, ui, &book)?,
            None if self.token == CARD => commands.run_action("core.library")?,
            None => return Ok(false),
        }
        Ok(true)
    }
}

impl Widgets {
    /// A screen made an instance of `widget`; one that is none of ours is none
    /// of our business.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let Some(mine) = Mine::new(widget)? else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            shown: None,
            mine,
        });
        Ok(())
    }

    /// An instance is gone.
    pub fn dropped(&mut self, instance: Instance) {
        self.items.retain(|item| item.instance != instance);
    }

    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    /// Answers a call of the `core.widget` stage: the height of an instance, where it
    /// sits, its targets (none for the date and the clock: a tap on these does
    /// nothing; the card's and the shelf's books).
    ///
    /// # Errors
    /// [`Error::NotFound`] for an instance that is not ours here, [`Error::Unsupported`]
    /// for a call it does not answer; font and host refusals.
    pub fn stage(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let (height, least) = item.mine.measure(host, &self.listing, width)?;
                call.reply_size(u32::from(height), u32::from(least), &mut out)?
            }
            WidgetCall::Place { instance, rect } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                item.mine.place(to_rect(rect));
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                item.mine.place(to_rect(rect));
                let found = item.mine.targets(host, ui, &self.listing, to_rect(rect))?;
                call.reply_targets(&found, &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Draws the instance the frame is for.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an instance that is not ours here; font and host
    /// refusals, [`Error::NoMemory`].
    pub fn draw(&self, host: &Host<'_>, ui: &Ui, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let area = to_rect(frame.rect);
        item.mine.draw(host, ui, &self.listing, &mut frame, area)
    }

    /// A tap on a target of the card or the shelf opens the book it shows (the
    /// card with no book: the library). Anything else is not handled.
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn input(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        instance: Instance,
        target: u32,
        kind: WidgetInputKind,
    ) -> Result<bool, Error> {
        match self.find(instance.get()) {
            Some(item) => item.mine.tap(host, ui, &self.listing, target, kind),
            None => Ok(false),
        }
    }

    /// A call into the plugin that is no draw: the books the card and the shelf
    /// kept are asked of the library again at their next draw.
    pub fn fresh(&self) {
        self.listing.fresh();
        self.listing.new_epoch();

        for item in &self.items {
            item.mine.known.fresh();
        }
    }

    /// A press that only moved the focus: the next frame draws the books the
    /// last one listed, with no question to the library.
    pub fn focus_step(&self) {
        self.listing.keep_for_next_frame();
    }

    /// A setting or the registry changed: what a widget kept of its look is read
    /// again at its next draw.
    pub fn forget_looks(&self) {
        for item in &self.items {
            item.mine.forget_look();
        }
    }

    pub fn covers_ready(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let ready = item.mine.covers_ready();
            if ready {
                self.listing.forget_covers();
            }
            if ready && commands.widget_changed(item.instance).is_err() {
                item.mine.waiting.set(true);
            }
        }
    }

    /// The minute came: tells the makers whose widget shows something new since
    /// the last minute. A widget that fails (its time or its command) is left for
    /// the next tick and costs the others nothing.
    pub fn tick(&mut self, host: &Host<'_>, ui: &Ui) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let now = match item.mine.token {
                DATE => native_date::shows(host),
                CARD => native_card::shows(host),
                SHELF => native_shelf::shows(host, &item.mine.widget),
                STATUS_WIDGET => Cx::open(host, &item.mine.widget)
                    .and_then(|cx| native_status::shows(&cx, ui, &item.mine.look)),
                _ => native_clock::shows(host),
            };
            let Ok(now) = now else {
                continue;
            };
            let instance = item.instance;
            // A cover that was being made is drawn again at the next minute, whatever
            // the books show.
            if item.shown.is_some_and(|before| before != now) {
                // The books changed: the card is asked about their files anew.
                item.mine.known.forget(&self.listing);
            }
            let waited = item.mine.waiting.replace(false);
            if waited {
                // A cover that was being made is asked about anew.
                self.listing.forget_covers();
            }
            if (waited || item.shown.is_some_and(|before| before != now))
                && commands.widget_changed(instance).is_err()
            {
                item.mine.waiting.set(waited);
                continue;
            }
            item.shown = Some(now);
        }
    }
}

/// A height that is its own least.
const fn same(height: u16) -> (u16, u16) {
    (height, height)
}

/// Opens `book` in the reader the user bound to `core.reader`: a hand-over with
/// its path (`document.md` section 1). A book gone from the catalog is not opened.
fn open_book(host: &Host<'_>, ui: &Ui, book: &native_recent::Book) -> Result<(), Error> {
    use paperpaw_sdk::native::library::BookRef;
    let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
    let path = native_recent::with_library(host, |library| {
        let reference = BookRef::catalog(book.key, book.generation)?;
        match library.book(host, reference, &mut input, &mut out) {
            Ok(found) => copy(found.path()).map(Some),
            Err(Error::State | Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    })?;
    let Some(path) = path else {
        return Ok(());
    };
    let params = [Param::new("path", &path)?];
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
}

/// The widget rectangle the host gave, as the kit's.
fn to_rect(rect: paperpaw_sdk::native::callbacks::FrameRect) -> Rect {
    let clamp = paperpaw_ui_kit::geometry::clamp_u16;
    Rect::new(
        clamp(i64::from(rect.x)),
        clamp(i64::from(rect.y)),
        clamp(i64::from(rect.width)),
        clamp(i64::from(rect.height)),
    )
}

/// A tap on an item of the status line that has an action of its owner's: run it.
fn status_tap(host: &Host<'_>, ui: &Ui, item: &Mine, target: u32) -> Result<bool, Error> {
    let Some(place) = item.placed.get() else {
        return Ok(false);
    };
    let cx = Cx::open(host, &item.widget)?;
    let inner = cx.shape.look(place).area;
    let Some(action) = native_status::action(&cx, ui, &item.look, inner, target)? else {
        return Ok(false);
    };
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).run_action(&action)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fail_alloc::sweep;

    #[test]
    fn the_widgets_of_home_are_ours_wherever_they_are_placed() {
        for id in [
            "home.date",
            "home.clock@home.page-1/0",
            "home.continue",
            "home.recent@2",
            "home.status",
        ] {
            assert!(Mine::new(id).unwrap().is_some(), "{id}");
        }
        for id in ["other.widget", "home.unknown", ""] {
            assert!(Mine::new(id).unwrap().is_none(), "{id}");
        }
    }

    #[test]
    fn making_one_of_ours_may_be_refused_for_memory_at_every_step() {
        sweep(|| Mine::new("home.clock@home.page-2/1").map(drop));
    }
}

#[cfg(test)]
#[path = "native_cover_tests.rs"]
mod cover_event_tests;
