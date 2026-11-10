//! A picture frame on interface 7: a picture from a folder on the card (JPEG,
//! PNG, GIF), another each day or each time it shows, the whole of it or its
//! place filled; its name below, if wanted. A tap shows the next one.
//!
//! The picture is a file the host decodes and draws (`docs/abi/document.md` 5):
//! the plugin lists the folder (`core.files`, counting the pictures and reading
//! the name of the chosen one, never keeping the names), asks the host to
//! prepare that file, holds the picture it answers with (one for each widget
//! placed) and draws it over the frame. What to ask and what an answer means
//! need no host: [`Pictures`] decides; `native_plugin` sends the commands.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        formatting::Arg,
        image::{ImageId, ImageStatus},
    },
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{clamp_u16, line_px},
    mem,
    text::copy,
    widget::{Align, Cx, Settings, layout::label},
};

use crate::native_draw::message_in;

use crate::native_quote::{Turns, daily, of_the_day};

/// The widget's token, as declared.
pub const TOKEN: u32 = 122;
/// Where the turns of the pictures are kept (see [`Turns`]).
const TURNS_FILE: &str = "picture.txt";
/// The longest path of a request (`command.image_prepare`).
pub const PATH_MAX: usize = 512;
/// The most bytes a folder setting holds.
pub const FOLDER_MAX: usize = PATH_MAX;
/// The endings of the pictures shown.
const PICTURES: [&str; 4] = [".jpg", ".jpeg", ".png", ".gif"];
/// What the cookies of this widget's requests begin with: the rest counts
/// the requests.
const COOKIE: u32 = 0x5049_0000;
/// The values of the setting of how the picture takes its place.
const FILL: &str = "widgets.picture.fit.fill";

/// Whether `cookie` is one of this widget's requests.
#[must_use]
pub const fn is_cookie(cookie: u32) -> bool {
    cookie & 0xffff_0000 == COOKIE
}

/// What a refusal of the host means for the picture of a widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// There is none: the folder is missing or not granted, or nothing here
    /// can be asked for. Not tried again until something changes.
    None,
    /// For now: the card or the service is busy, the host has no room for
    /// another image (it prepares 8 and queues 4 for a plugin, and a widget
    /// holds two while the next loads). Chosen again at the next minute.
    Later,
}

/// What the refusal `error` means: only a missing, forbidden or unsupported
/// folder, or a request that cannot be made, means there is no picture.
#[must_use]
pub const fn refusal(error: Error) -> Refusal {
    match error {
        Error::NotFound | Error::Denied | Error::Unsupported | Error::Invalid => Refusal::None,
        _ => Refusal::Later,
    }
}

/// Whether `name` is a picture shown: a file with one of the endings, not
/// hidden.
#[must_use]
pub fn is_picture(name: &str, is_dir: bool) -> bool {
    !is_dir
        && !name.starts_with('.')
        && PICTURES.iter().any(|ending| {
            name.len()
                .checked_sub(ending.len())
                .and_then(|from| name.get(from..))
                .is_some_and(|tail| tail.eq_ignore_ascii_case(ending))
        })
}

/// A picture's name as a caption: without its ending, `_` as spaces.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn caption_of(name: &str) -> Result<String, Error> {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let mut out = String::new();
    out.try_reserve_exact(stem.len())
        .map_err(|_| Error::NoMemory)?;
    out.extend(stem.chars().map(|c| if c == '_' { ' ' } else { c }));
    let kept = out.trim();
    if kept.len() == out.len() {
        Ok(out)
    } else {
        copy(kept)
    }
}

/// A folder, as the plugin walks it: each entry's name and whether it is a
/// folder, in the order the card keeps them.
pub trait Folder {
    /// Calls `visit` for every entry.
    ///
    /// # Errors
    /// What the host refuses.
    fn each(&mut self, visit: &mut dyn FnMut(&str, bool)) -> Result<(), Error>;
}

/// The name of the picture `position(count)` (taken modulo `count`) of the
/// `count` in `folder`, as the folder lists them: counted, not kept (a folder
/// may hold thousands); `None` where there is none.
///
/// # Errors
/// [`Error::NoMemory`], or what the folder refuses.
pub fn picture_at(
    folder: &mut impl Folder,
    position: impl Fn(usize) -> usize,
) -> Result<Option<String>, Error> {
    let mut count = 0usize;
    folder.each(&mut |name, is_dir| count += usize::from(is_picture(name, is_dir)))?;
    if count == 0 {
        return Ok(None);
    }
    let at = position(count) % count;
    let mut index = 0usize;
    let mut found: Option<Result<String, Error>> = None;
    folder.each(&mut |name, is_dir| {
        if !is_picture(name, is_dir) {
            return;
        }
        if index == at {
            found = Some(copy(name));
        }
        index += 1;
    })?;
    found.transpose()
}

/// What the settings, the day and the turns make the picture of a widget: a
/// number that changes when another is to be chosen.
///
/// # Errors
/// Host refusals.
pub fn mark(
    host: &Host<'_>,
    widget: &str,
    showings: &crate::native_quote::Showings,
) -> Result<u64, Error> {
    let settings = Settings::new(host, widget);
    let daily = daily(&settings)?;
    let day = crate::native_draw::epoch_day(host)?;
    let showing = if daily { 0 } else { showings.count() };
    let turns = Turns::load(host, TURNS_FILE)?
        .on(day, daily)
        .wrapping_add(showing);
    let mut mark = crate::native_draw::fnv(1, &[u8::from(daily)]);
    mark = crate::native_draw::fnv(mark, folder_of(&settings)?.as_bytes());
    mark = crate::native_draw::fnv(mark, &turns.to_le_bytes());
    if daily {
        mark = crate::native_draw::fnv(mark, &day.to_le_bytes());
    }
    Ok(mark)
}

/// The folder of the pictures, without slashes at its ends.
///
/// # Errors
/// Host refusals.
pub fn folder_of(settings: &Settings<'_, '_>) -> Result<String, Error> {
    let folder = settings.string("folder", FOLDER_MAX)?;
    copy(folder.trim().trim_matches('/'))
}

/// Which picture of `count` the widget shows now.
///
/// # Errors
/// Host refusals.
pub fn position(
    host: &Host<'_>,
    widget: &str,
    showings: &crate::native_quote::Showings,
) -> Result<impl Fn(usize) -> usize + use<>, Error> {
    let settings = Settings::new(host, widget);
    let daily = daily(&settings)?;
    let day = crate::native_draw::epoch_day(host)?;
    let showing = if daily { 0 } else { showings.count() };
    let turns = Turns::load(host, TURNS_FILE)?
        .on(day, daily)
        .wrapping_add(showing);
    Ok(move |count: usize| {
        let first = if daily { of_the_day(day, count) } else { 0 };
        first.wrapping_add(turns as usize)
    })
}

/// Turns the picture of the widget `widget` on to the next.
///
/// # Errors
/// The host's refusals.
pub fn turn(host: &Host<'_>, widget: &str) -> Result<(), Error> {
    let settings = Settings::new(host, widget);
    let day = crate::native_draw::epoch_day(host)?;
    let turns = Turns::load(host, TURNS_FILE)?.on(day, daily(&settings)?);
    Turns {
        day,
        turns: turns.wrapping_add(1),
    }
    .save(host, TURNS_FILE)
}

/// A picture held for a widget.
#[derive(Debug)]
pub struct Held {
    pub image: ImageId,
    /// Its size as the host made it.
    pub size: (u32, u32),
    /// Its name as a caption.
    pub caption: String,
}

/// What the host's answer to a request leads to.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Not the answer to a request open.
    Ignore,
    /// The picture of widget `instance` is ready; the one held before is to be
    /// released.
    Ready {
        instance: u32,
        release: Option<ImageId>,
    },
    /// The file cannot be shown: the widget has none now (its held picture is
    /// to be released).
    Gone {
        instance: u32,
        release: Option<ImageId>,
    },
    /// Refused for want of memory or the card: chosen again at the next
    /// minute.
    Retry,
}

/// The pictures of the widgets placed and what is asked for them.
#[derive(Debug, Default)]
pub struct Pictures {
    slots: Vec<Slot>,
    /// Counts the requests of all widgets, so that every request has a cookie
    /// of its own and an answer to an older one is not taken for a later one's.
    counter: u16,
}

#[derive(Debug)]
struct Slot {
    instance: u32,
    /// What the picture held, asked for or found missing was chosen for.
    mark: Option<u64>,
    held: Option<Held>,
    /// The request open: its cookie and the caption of the file.
    open: Option<(u32, String)>,
    /// Nothing to show for the folder (until another is chosen).
    empty: bool,
}

/// How a widget's picture is, for drawing.
#[derive(Debug, Clone, Copy)]
pub enum State<'a> {
    /// A picture is asked for; the widget waits for it.
    Loading,
    /// The folder holds none (or it cannot be read).
    Empty,
    /// The picture.
    Shown(&'a Held),
}

impl Pictures {
    /// A widget was made.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn made(&mut self, instance: u32) -> Result<(), Error> {
        if self.slots.iter().any(|slot| slot.instance == instance) {
            return Ok(());
        }
        self.slots.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.slots.push(Slot {
            instance,
            mark: None,
            held: None,
            open: None,
            empty: false,
        });
        Ok(())
    }

    /// A widget is gone: the picture it held is to be released.
    pub fn dropped(&mut self, instance: u32) -> Option<ImageId> {
        let at = self
            .slots
            .iter()
            .position(|slot| slot.instance == instance)?;
        let slot = self.slots.remove(at);
        slot.held.map(|held| held.image)
    }

    fn slot_mut(&mut self, instance: u32) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|slot| slot.instance == instance)
    }

    /// How the picture of `instance` is for drawing.
    #[must_use]
    pub fn state(&self, instance: u32) -> State<'_> {
        match self.slots.iter().find(|slot| slot.instance == instance) {
            Some(Slot {
                held: Some(held), ..
            }) => State::Shown(held),
            Some(Slot { empty: true, .. }) => State::Empty,
            _ => State::Loading,
        }
    }

    /// Whether `instance` waits to be chosen a picture for `mark`.
    #[must_use]
    pub fn wants(&self, instance: u32, mark: u64) -> bool {
        self.slots
            .iter()
            .find(|slot| slot.instance == instance)
            .is_some_and(|slot| slot.mark != Some(mark))
    }

    /// The request for the picture `caption` of `instance`, chosen for `mark`,
    /// is about to be made: its cookie.
    pub fn asking(&mut self, instance: u32, mark: u64, caption: String) -> Option<u32> {
        self.counter = self.counter.wrapping_add(1);
        let cookie = COOKIE | u32::from(self.counter);
        let slot = self.slot_mut(instance)?;
        slot.mark = Some(mark);
        slot.empty = false;
        slot.open = Some((cookie, caption));
        Some(cookie)
    }

    /// The request was refused: it is chosen again at the next minute.
    pub fn refused(&mut self, instance: u32) {
        if let Some(slot) = self.slot_mut(instance) {
            slot.mark = None;
            slot.open = None;
        }
    }

    /// The folder holds no picture for `mark`: what was held is to be released.
    pub fn none(&mut self, instance: u32, mark: u64) -> Option<ImageId> {
        let slot = self.slot_mut(instance)?;
        slot.mark = Some(mark);
        slot.open = None;
        slot.empty = true;
        slot.held.take().map(|held| held.image)
    }

    /// The host's answer: its cookie, status, picture and size.
    pub fn ready(
        &mut self,
        (cookie, status, image, size): (u32, ImageStatus, Option<ImageId>, (u32, u32)),
    ) -> Next {
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.open.as_ref().is_some_and(|(open, _)| *open == cookie))
        else {
            return Next::Ignore;
        };
        let (_, caption) = slot.open.take().unwrap_or_default();
        let instance = slot.instance;
        match (status, image) {
            (ImageStatus::Ready, Some(image)) => {
                let release = slot.held.take().map(|held| held.image);
                slot.held = Some(Held {
                    image,
                    size,
                    caption,
                });
                Next::Ready { instance, release }
            }
            (ImageStatus::NoMemory | ImageStatus::CardFailure, _) => {
                slot.mark = None;
                Next::Retry
            }
            _ => {
                slot.empty = true;
                let release = slot.held.take().map(|held| held.image);
                Next::Gone { instance, release }
            }
        }
    }
}

/// The height of the caption below the picture with its gap; 0 without.
fn caption_height(cx: &Cx<'_>) -> Result<u16, Error> {
    if !cx.settings.shows("caption")? {
        return Ok(0);
    }
    let small = line_px(&cx.font, cx.style().small)?;
    Ok(small.saturating_add(cx.style().gap / 2))
}

/// What the settings make drawn change, as a number.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let settings = Settings::new(host, widget);
    let mark = crate::native_draw::fnv(1, &[u8::from(settings.shows("caption")?)]);
    Ok(crate::native_draw::fnv(
        mark,
        settings.string("fit", 96)?.as_bytes(),
    ))
}

/// The height: as high as wide (a third of the screen at most), the caption
/// and the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let side = width.min(cx.snapshot.display_height() / 3);
    Ok(side
        .saturating_add(caption_height(cx)?)
        .saturating_add(cx.shape.frame_height()))
}

/// Where the picture of `size` goes in `room` and how it fills it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub image: ImageId,
    pub rect: Rect,
    /// Fills the rectangle, its edges cut.
    pub fill: bool,
}

/// The size of a picture `size` shown as large as fits in `room`, its
/// proportions kept.
#[must_use]
pub fn fit_in(size: (u32, u32), room: (u16, u16)) -> (u16, u16) {
    let (w, h) = (u64::from(size.0), u64::from(size.1));
    let (rw, rh) = (u64::from(room.0), u64::from(room.1));
    if w == 0 || h == 0 || rw == 0 || rh == 0 {
        return (0, 0);
    }
    let (fw, fh) = if w * rh > h * rw {
        (rw, (rw * h / w).max(1))
    } else {
        ((rh * w / h).max(1), rh)
    };
    (
        clamp_u16(i64::try_from(fw).unwrap_or(i64::MAX)),
        clamp_u16(i64::try_from(fh).unwrap_or(i64::MAX)),
    )
}

/// Draws the widget in `area` as far as paint goes (the frame, the caption or
/// the message) and says where the picture goes: the host draws it over the
/// paint.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_with(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    state: State<'_>,
) -> Result<Option<Placed>, Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let held = match state {
        State::Loading => return Ok(None),
        State::Empty => {
            let folder = folder_of(&cx.settings)?;
            let message = cx.format(
                "widgets.picture.none",
                &[Arg::Text {
                    name: "folder",
                    value: &folder,
                }],
            )?;
            message_in(cx, out, &look, &message)?;
            return Ok(None);
        }
        State::Shown(held) => held,
    };
    let area = look.area;
    // The caption where the picture keeps most of the room.
    let below = Some(caption_height(cx)?)
        .filter(|&h| u32::from(h) * 3 <= u32::from(area.height))
        .unwrap_or(0);
    let room = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(below),
    );
    let fill = cx.settings.string("fit", 96)? == FILL;
    let place = if fill {
        room
    } else {
        let (w, h) = fit_in(held.size, (room.width, room.height));
        Rect::new(
            room.x.saturating_add(room.width.saturating_sub(w) / 2),
            room.y.saturating_add(room.height.saturating_sub(h) / 2),
            w,
            h,
        )
    };
    if below > 0 && !held.caption.is_empty() {
        let small = cx.style().small;
        let line = line_px(&cx.font, small)?;
        // Just below the picture, wherever its whole leaves it.
        let top = place
            .y
            .saturating_add(place.height)
            .saturating_add(below)
            .saturating_sub(line);
        let align: Align = cx.settings.align()?;
        let row = Rect::new(area.x, top, area.width, line);
        label(
            out,
            &cx.font,
            (small, align),
            row,
            &held.caption,
            look.faint,
        )?;
    }
    Ok((place.width > 0 && place.height > 0).then_some(Placed {
        image: held.image,
        rect: place,
        fill,
    }))
}

/// A buffer pair for listing a folder, made once.
pub fn buffers() -> Result<(Vec<u8>, Vec<u8>), Error> {
    use paperpaw_sdk::native::files::{INPUT_BYTES, OUTPUT_BYTES};
    Ok((mem::buffer(INPUT_BYTES)?, mem::buffer(OUTPUT_BYTES)?))
}

#[cfg(test)]
mod tests;
