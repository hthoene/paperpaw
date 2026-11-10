//! The vocabulary's home-screen widgets on interface 7 (interface 6:
//! `widget.rs`): *Word of the day*, *Words saved* and *Cards due*. The host
//! makes an instance when a screen shows one (`event.widget_made`), asks it for
//! its height through the `core.widget` stage, and calls the plugin to draw it
//! (`DrawPurpose::Widget`, `frame.id` the instance). Nothing of a callback is
//! kept: an instance is the widget's id and a seed, and what it shows is read
//! from the files of words and cards when it is measured or drawn. A tap on the
//! first two opens the list of words, on the third the flash cards.
//!
//! The pieces the widgets share (the caption, the sizes, the empty message) are
//! here; each widget is a module of its own.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, FrameRect, Input};
use paperpaw_sdk::native::reply::Reply;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::ui::{Instance, WidgetInputKind, WidgetTarget, stages::WidgetCall};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Rect, StorageError};
use paperpaw_ui_kit::widget::layout::{caption, caption_height};
use paperpaw_ui_kit::widget::settings::{Settings, base};
use paperpaw_ui_kit::widget::text::{Wrapped, largest_size};
use paperpaw_ui_kit::widget::{Align, Cx, Look, TextSize};
use paperpaw_ui_kit::{Paint, geometry, paint};

use crate::declarations::{DUE_WIDGET_TOKEN, PICKS, SAVED_WIDGET_TOKEN, WORD_WIDGET_TOKEN};
use crate::files::DataFolder;
use crate::native_review::storage_error;
use crate::store;

/// The most bytes of a stage reply (a size, or one target).
const REPLY: usize = 256;
/// The target of a tap: the whole widget.
const WHOLE: u32 = 1;

/// The widget ids, as the host names them (a placement may follow).
pub(crate) const WORD_ID: &str = "vocabulary.widget.word";
pub(crate) const SAVED_ID: &str = "vocabulary.widget.saved";
pub(crate) const DUE_ID: &str = "vocabulary.widget.due";

/// What each widget does, by the token it was declared with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Behaviour {
    /// The natural height; 0 shows nothing.
    pub height: fn(&mut Cx<'_>) -> Result<u16, Error>,
    /// Lays the widget out in a rectangle; the last is the instance's seed.
    pub draw: fn(&mut Cx<'_>, &mut Vec<Paint>, Rect, u32) -> Result<(), Error>,
}

static WORD: Behaviour = Behaviour {
    height: crate::native_widget_word::height,
    draw: crate::native_widget_word::draw,
};
static SAVED: Behaviour = Behaviour {
    height: crate::native_widget_saved::height,
    draw: crate::native_widget_saved::draw,
};
static DUE: Behaviour = Behaviour {
    height: crate::native_widget_due::height,
    draw: crate::native_widget_due::draw,
};

/// The behaviour of the widget declared with `token`.
fn behaviour(token: u32) -> Option<&'static Behaviour> {
    match token {
        WORD_WIDGET_TOKEN => Some(&WORD),
        SAVED_WIDGET_TOKEN => Some(&SAVED),
        DUE_WIDGET_TOKEN => Some(&DUE),
        _ => None,
    }
}

/// The token of the widget `widget` (its id, with a placement perhaps).
fn token_of(widget: &str) -> Option<u32> {
    match base(widget) {
        WORD_ID => Some(WORD_WIDGET_TOKEN),
        SAVED_ID => Some(SAVED_WIDGET_TOKEN),
        DUE_ID => Some(DUE_WIDGET_TOKEN),
        _ => None,
    }
}

/// What a tap on a widget opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tap {
    /// The list of words.
    Words,
    /// The flash cards.
    Cards,
}

/// One widget made for a screen.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when it was last asked about, for the tick.
    shown: Option<u64>,
    /// Picks the word of the day where the device does not know the day: the
    /// same for the life of the instance, so that measuring and drawing agree.
    seed: u32,
}

/// The widgets made for screens, by the host's instance.
#[derive(Debug, Default)]
pub(crate) struct Widgets {
    items: Vec<Item>,
}

impl Widgets {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    /// A screen shows the widget `widget`: it gets an instance. A widget this
    /// plugin does not make is none of its business.
    pub(crate) fn made(
        &mut self,
        host: &Host<'_>,
        widget: &str,
        instance: Instance,
    ) -> Result<(), Error> {
        let Some(token) = token_of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        // The clock of the device, as far as it goes: any number will do.
        let seed = paperpaw_sdk::native::clock::micros(host).map_or(0, |micros| micros as u32);
        self.items.push(Item {
            instance,
            token,
            widget: paperpaw_ui_kit::text::copy(widget)?,
            shown: None,
            seed,
        });
        Ok(())
    }

    /// The screen dropped the instance.
    pub(crate) fn dropped(&mut self, instance: Instance) {
        self.items.retain(|item| item.instance != instance);
    }

    /// Answers a call of the stage `core.widget`: the height, the place kept,
    /// the one target (the whole widget).
    pub(crate) fn stage(
        &self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = paperpaw_ui_kit::mem::buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, .. } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let height = (behaviour.height)(&mut cx)?;
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_targets(&[WidgetTarget::new(WHOLE, rect)], &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Draws the instance `frame.id` is.
    pub(crate) fn draw(&self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = area_of(frame.rect);
        let mut out = Vec::new();
        (behaviour.draw)(&mut cx, &mut out, area, item.seed)?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    /// What `input` opens, if it is a tap on one of the widgets.
    pub(crate) fn tapped(&self, input: &Input<'_>) -> Option<Tap> {
        let Input::Widget {
            instance,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return None;
        };
        Some(if self.find(instance.get())?.token == DUE_WIDGET_TOKEN {
            Tap::Cards
        } else {
            Tap::Words
        })
    }

    /// Tells the host to ask the widgets again: the words changed.
    pub(crate) fn changed(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = paperpaw_sdk::native::commands::CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            // One that fails is left for the next tick.
            if commands.widget_changed(item.instance).is_err() {
                item.shown = None;
            }
        }
    }

    /// Tells the host about the widgets that show something new since the last
    /// minute: another day, more words, another number of cards due. One that
    /// fails is left for the next tick and costs the others nothing.
    pub(crate) fn tick(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = paperpaw_sdk::native::commands::CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let Ok(now) = stamp(host, item) else {
                continue;
            };
            let instance = item.instance;
            if item.shown.is_some_and(|before| before != now)
                && commands.widget_changed(instance).is_err()
            {
                continue;
            }
            item.shown = Some(now);
        }
    }
}

/// What a widget shows, in one number: it differs when the widget would.
fn stamp(host: &Host<'_>, item: &Item) -> Result<u64, Error> {
    let data = DataStorage::new(host);
    let folder = DataFolder(&data);
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let parts: [u64; 4] = match item.token {
        WORD_WIDGET_TOKEN => {
            let newest = Settings::new(host, &item.widget).with_text("pick", |v| v == PICKS[1])?;
            let day = crate::native_widget_word::today_of(host)?;
            [
                day.map_or(0, |day| day as u64 + 1),
                store::size(&folder),
                u64::from(newest),
                0,
            ]
        }
        // The size of the file of words changes with any word added, deleted or
        // replaced: no pass over the file, once a minute for each widget.
        SAVED_WIDGET_TOKEN => [store::size(&folder), 0, 0, 0],
        _ => [
            crate::native_widget_due::due(host)?.map_or(0, |due| due as u64 + 1),
            0,
            0,
            0,
        ],
    };
    for part in parts {
        hash = (hash ^ part).wrapping_mul(0x0100_0000_01b3);
    }
    Ok(hash)
}

/// What reading the files says: a file that cannot be read shows as none (as
/// interface 6 did), a refused allocation is [`Error::NoMemory`].
pub(crate) fn read<T: Default>(result: Result<T, StorageError>) -> Result<T, Error> {
    match result {
        Ok(value) => Ok(value),
        Err(error) if storage_error(error) == Error::NoMemory => Err(Error::NoMemory),
        Err(_) => Ok(T::default()),
    }
}

/// The rectangle of a frame as the drawing area.
fn area_of(rect: FrameRect) -> Rect {
    Rect::new(
        geometry::clamp_u16(i64::from(rect.x)),
        geometry::clamp_u16(i64::from(rect.y)),
        geometry::clamp_u16(i64::from(rect.width)),
        geometry::clamp_u16(i64::from(rect.height)),
    )
}

/// The height a widget's caption takes, if it shows one (the part `caption`).
///
/// # Errors
/// Font and host refusals.
pub(crate) fn caption_room(cx: &Cx<'_>) -> Result<u16, Error> {
    if cx.settings.shows("caption")? {
        caption_height(&cx.font, cx.style().small, cx.style().gap)
    } else {
        Ok(0)
    }
}

/// The caption, if the widget shows it, it fits the width whole and there is
/// room for a line of text below it as well; else none: the content goes first,
/// and the caption is left out rather than cut.
///
/// # Errors
/// Font and host refusals.
pub(crate) fn pick_caption<'t>(
    cx: &Cx<'_>,
    area: Rect,
    text: &'t str,
) -> Result<Option<&'t str>, Error> {
    let room = caption_room(cx)?;
    let least = geometry::line_px(&cx.font, cx.style().text)?;
    if room == 0 || room.saturating_add(least) > area.height {
        return Ok(None);
    }
    if cx.font.measure_text(cx.style().small, text)? <= f32::from(area.width) {
        Ok(Some(text))
    } else {
        Ok(None)
    }
}

/// Places content `height` high with the `caption` (if any) above it, the two
/// together in the middle of `look`'s area, and draws the caption; where the
/// content goes. Where the two do not fit together, the caption is left out.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub(crate) fn captioned(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    text: Option<&str>,
    height: u16,
) -> Result<Rect, Error> {
    let cap = caption_height(&cx.font, cx.style().small, cx.style().gap)?;
    let text = text.filter(|_| cap.saturating_add(height) <= look.area.height);
    let cap = if text.is_some() { cap } else { 0 };
    let total = cap.saturating_add(height).min(look.area.height);
    let block = Rect::new(
        look.area.x,
        look.area
            .y
            .saturating_add(look.area.height.saturating_sub(total) / 2),
        look.area.width,
        total,
    );
    if let Some(text) = text {
        caption(
            out,
            &cx.font,
            (cx.style().small, cx.style().gap),
            (block, cx.settings.align()?, look.faint),
            text,
        )?;
    }
    Ok(Rect::new(
        block.x,
        block.y.saturating_add(cap),
        block.width,
        height.min(total.saturating_sub(cap)),
    ))
}

/// The text size the widget's size setting gives, smaller where `text` is
/// wider than `width` (down to the body's size, below which it is shortened)
/// or a line of it higher than `height` (down to the small size); "as large as
/// fits" fits it into both, as large as they hold.
///
/// # Errors
/// Font and host refusals.
pub(crate) fn text_size(cx: &Cx<'_>, text: &str, width: u16, height: u16) -> Result<f32, Error> {
    let style = cx.style();
    let max = match cx.settings.size()? {
        TextSize::Fit => f32::from(height).max(style.text),
        size => size.text(style),
    };
    let wide = f32::from(width);
    let tall =
        |size: f32| -> Result<bool, Error> { Ok(geometry::line_px(&cx.font, size)? <= height) };
    let least = style.text.min(max);
    let found = largest_size(max, least, |size| {
        Ok(tall(size)? && cx.font.measure_text(size, text)? <= wide)
    })?;
    if let Some(size) = found {
        return Ok(size);
    }
    Ok(largest_size(max.min(style.text), style.small, tall)?.unwrap_or(style.small.min(max)))
}

/// `text` where it fits `width` at `size`, or the number it begins with alone
/// ("128 words": "128").
///
/// # Errors
/// Font refusals.
pub(crate) fn number_if_narrow<'t>(
    cx: &Cx<'_>,
    size: f32,
    text: &'t str,
    width: u16,
) -> Result<&'t str, Error> {
    if cx.font.measure_text(size, text)? <= f32::from(width) {
        return Ok(text);
    }
    let digits = text
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map_or(text.len(), |(at, _)| at);
    Ok(if digits == 0 {
        text
    } else {
        text.get(..digits).unwrap_or(text)
    })
}

/// What a widget says without words in `area` (its whole place): the long text
/// inside the spacing, or where that does not fit whole, the short one.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub(crate) fn draw_empty(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    (long, short): (&str, &str),
) -> Result<(), Error> {
    let (text, small) = (cx.style().text, cx.style().small);
    // A short message as wide as the spacing leaves, as high as half of it: the
    // lines' own room above and below the letters keeps them clear of the edges
    // (two lines fit a low place then).
    let inner = look.area;
    let (width, height) = (inner.width, (inner.height.saturating_add(area.height)) / 2);
    let long = cx.tr(long)?;
    let mut wrapped =
        Wrapped::fitting(&cx.font, &long, (text, small), (width, inner.height), false)?;
    if !wrapped.whole() {
        let short = cx.tr(short)?;
        wrapped = Wrapped::fitting(&cx.font, &short, (text, small), (width, height), false)?;
    }
    let at = Rect::new(inner.x, area.y, inner.width, area.height);
    wrapped.draw(out, &cx.font, at, Align::Center, look.ink)?;
    Ok(())
}
