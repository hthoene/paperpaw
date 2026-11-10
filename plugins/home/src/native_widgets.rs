//! The home screen's own widgets on the flat native boundary: the host makes an
//! instance when a screen shows one, asks it for its height and where it sits,
//! and calls the plugin to draw it. Nothing of a callback is kept; an instance
//! is the widget's id with its placement. The widgets are the date, the clock,
//! the continue card and the recent shelf; the status line follows
//! (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use core::cell::Cell;
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        commands::CommandSender,
        fonts::{Font, FontRole},
        image::ImageStyle,
        reply::Reply,
        ui::{Instance, Open, Param, Ui, WidgetInputKind, WidgetTarget, stages::WidgetCall},
    },
};
use paperpaw_ui_kit::{icons, mem::buffer, paint, text::copy, widget::Cx};

use crate::declarations::{CARD, CLOCK, DATE, SHELF, STATUS_WIDGET};
use crate::native_recent::{self, Known, Pictures};
use crate::{native_card, native_clock, native_date, native_shelf, native_status};

/// The most bytes of a stage reply (sizes and no targets).
const REPLY: usize = 256;

/// The widget an instance is.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when the tick last looked; `None` until the first tick,
    /// which only records it (the draw does not set it).
    shown: Option<u64>,
    /// A cover it drew was still being made: the next tick asks the screen to
    /// draw it again.
    waiting: Cell<bool>,
    /// The books of the card or the shelf whose files the host found gone.
    known: Known,
    /// Where the screen last placed it, for a tap on one of its items.
    placed: Cell<Option<Rect>>,
}

/// The instances the screens made of the widgets.
#[derive(Debug, Default)]
pub struct Widgets {
    items: Vec<Item>,
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

impl Widgets {
    /// A screen made an instance of `widget`; one that is none of ours is none
    /// of our business.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let Some(token) = token_of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: copy(widget)?,
            shown: None,
            waiting: Cell::new(false),
            known: Known::default(),
            placed: Cell::new(None),
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
                let mut cx = Cx::open(host, &item.widget)?;
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let (height, least) = match item.token {
                    DATE => same(native_date::height(&mut cx, width)?),
                    CARD => (native_card::height(&cx), native_card::height(&cx) / 2),
                    SHELF => native_shelf::height(&cx, &item.known, width)?,
                    STATUS_WIDGET => same(native_status::height(&cx)?),
                    _ => same(native_clock::height(&mut cx, width)?),
                };
                call.reply_size(u32::from(height), u32::from(least), &mut out)?
            }
            WidgetCall::Place { instance, rect } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                item.placed.set(Some(to_rect(rect)));
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                item.placed.set(Some(to_rect(rect)));
                let mut found: Vec<WidgetTarget> = Vec::new();
                match item.token {
                    STATUS_WIDGET => {
                        let cx = Cx::open(host, &item.widget)?;
                        let inner = cx.shape.look(to_rect(rect)).area;
                        for (id, at) in native_status::targets(&cx, ui, inner)? {
                            let target = WidgetTarget::new(id, paint::frame_rect(at));
                            paperpaw_ui_kit::mem::push(&mut found, target)?;
                        }
                    }
                    CARD => paperpaw_ui_kit::mem::push(
                        &mut found,
                        WidgetTarget::new(native_card::TARGET, rect),
                    )?,
                    SHELF => {
                        let mut cx = Cx::open(host, &item.widget)?;
                        let clamp = paperpaw_ui_kit::geometry::clamp_u16;
                        let area = Rect::new(
                            clamp(i64::from(rect.x)),
                            clamp(i64::from(rect.y)),
                            clamp(i64::from(rect.width)),
                            clamp(i64::from(rect.height)),
                        );
                        for (id, at) in native_shelf::targets(&mut cx, &item.known, area)? {
                            let target = WidgetTarget::new(id, paint::frame_rect(at));
                            paperpaw_ui_kit::mem::push(&mut found, target)?;
                        }
                    }
                    _ => {}
                }
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
        if item.token == STATUS_WIDGET {
            return native_status::draw_widget(host, ui, &mut frame, &item.widget, area);
        }
        let mut cx = Cx::open(host, &item.widget)?;
        let mut out = Vec::new();
        let mut pictures = Pictures::default();
        let icons = if item.token == CARD {
            icons::font(host)?
        } else {
            None
        };
        match item.token {
            DATE => native_date::draw(&mut cx, &mut out, area)?,
            CARD => native_card::draw(
                &mut cx,
                &mut out,
                icons.as_ref(),
                (&mut pictures, &item.known),
                area,
            )?,
            SHELF => native_shelf::draw(&mut cx, &mut out, (&mut pictures, &item.known), area)?,
            _ => native_clock::draw(&mut cx, &mut out, area)?,
        }
        item.waiting.set(pictures.busy);
        let bold = if out
            .iter()
            .any(|op| matches!(op, paperpaw_ui_kit::Paint::Bold { .. }))
        {
            Some(Font::acquire_role(host, FontRole::UiBold)?)
        } else {
            None
        };
        paint::replay_all(&mut frame, &cx.font, (bold.as_ref(), icons.as_ref()), &out)?;
        // The covers go where nothing else is drawn. One the host no longer has
        // (it ended the id for room) or cannot draw is left out: the next draw asks
        // for it again.
        for (image, at) in &pictures.placed {
            frame
                .draw_canvas()?
                .draw_image(*image, paint::frame_rect(*at), ImageStyle::new().small())
                .ok();
        }
        Ok(())
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
        let Some(item) = self.find(instance.get()) else {
            return Ok(false);
        };
        if kind != WidgetInputKind::Tap || target == 0 {
            return Ok(false);
        }
        if item.token == STATUS_WIDGET {
            return status_tap(host, ui, item, target);
        }
        let book = match item.token {
            CARD => native_recent::read(host, &item.known, 0, 1)?
                .into_iter()
                .next(),
            SHELF => {
                let cx = Cx::open(host, &item.widget)?;
                let at = usize::try_from(target - 1).map_err(|_| Error::Invalid)?;
                native_shelf::books(&cx, &item.known)?.into_iter().nth(at)
            }
            _ => return Ok(false),
        };
        let mut scratch = [0; 256];
        let mut commands = CommandSender::new(host, &mut scratch);
        match book {
            Some(book) => open_book(host, ui, &book)?,
            None if item.token == CARD => commands.run_action("core.library")?,
            None => return Ok(false),
        }
        Ok(true)
    }

    /// The minute came: tells the makers whose widget shows something new since
    /// the last minute. A widget that fails (its time or its command) is left for
    /// the next tick and costs the others nothing.
    pub fn tick(&mut self, host: &Host<'_>, ui: &Ui) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let now = match item.token {
                DATE => native_date::shows(host),
                CARD => native_card::shows(host),
                SHELF => native_shelf::shows(host, &item.widget),
                STATUS_WIDGET => {
                    Cx::open(host, &item.widget).and_then(|cx| native_status::shows(&cx, ui))
                }
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
                item.known.forget();
            }
            let waited = item.waiting.replace(false);
            if (waited || item.shown.is_some_and(|before| before != now))
                && commands.widget_changed(instance).is_err()
            {
                item.waiting.set(waited);
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
fn status_tap(host: &Host<'_>, ui: &Ui, item: &Item, target: u32) -> Result<bool, Error> {
    let Some(place) = item.placed.get() else {
        return Ok(false);
    };
    let cx = Cx::open(host, &item.widget)?;
    let inner = cx.shape.look(place).area;
    let Some(action) = native_status::action(&cx, ui, inner, target)? else {
        return Ok(false);
    };
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).run_action(&action)?;
    Ok(true)
}
