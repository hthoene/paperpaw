//! The pictures of the sleep screen on interface 7: the sleeping cat and the
//! logo (embedded, written once into the data folder), the cover of the book
//! read last (the library's), the next picture of the folder and the picture
//! over the page (files of the card). The host decodes a file in the
//! background (`image_prepare`) and says when it is ready; a draw can send no
//! command, so everything is asked for from the events (when the screen opens,
//! and again when an answer says a file failed) and drawn from what is held.
//!
//! Until a picture is there the screen shows the message; when it comes the
//! screen asks for a full refresh. A file that fails (not there, damaged, no
//! consent for the card) clears its fact and the decision of
//! [`crate::native_sleep`] falls back: the picture to the cat, the cover to
//! the cat, the picture over the page to the cover.
use super::Sleeping;
use crate::native_pick::Failure;
use crate::native_sleep::{self as model, Content, Pictures};
use alloc::string::String;
use paperpaw_sdk::{
    Rect, Storage as _,
    native::{
        Error, Host,
        commands::CommandSender,
        files::Files,
        fonts::{Font, FontRole},
        image::{ImageId, ImageReady, ImageStatus, PrepareImage, PrepareRoot},
        library::{BookRef, Library, Query, Sort},
        snapshot::capture_snapshot,
        storage::{self, DataStorage, Root},
    },
};
use paperpaw_ui_kit::{
    Style,
    mem::{SERVICE_BUFFER, buffer},
};

/// The embedded files, as written into the data folder.
const CAT_FILE: &str = "cat.png";
const LOGO_FILE: &str = "logo.png";
const LOGO: &[u8] = include_bytes!("../../pictures/logo.png");
/// The count of pictures shown (`next.txt`, as interface 6 kept it).
const NEXT: &str = "next.txt";
/// Pages of the folder listing read at most.
const MOST_PAGES: usize = 8;
/// Steps a decision of what to ask for takes at most (each clears a fact).
const MOST_STEPS: usize = 6;

/// What a picture asked for is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Cat,
    Logo,
    Picture,
    Over,
}

/// A picture the host holds for the screen and its size in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Held {
    pub(super) id: ImageId,
    width: u32,
    height: u32,
}

/// The pictures of one screen.
#[derive(Debug, Default)]
pub(super) struct Pics {
    /// The cat, the logo, the folder's picture or the one over the page.
    main: Option<(Kind, Held)>,
    /// The cover (also the spare of the picture over the page).
    cover: Option<Held>,
    /// The request open: its cookie and what it is for.
    asked: Option<(u32, Kind)>,
    /// The cover was looked for.
    cover_tried: bool,
    /// The cat or the logo could not be made: the message stays alone.
    given_up: bool,
    /// The file asked for to go over the page (for why it did not show).
    over_path: String,
}

impl Sleeping {
    /// What the screen draws now, by the setting and the facts found so far.
    pub(super) fn content(&self) -> Content {
        let mut facts = self.facts;
        if let Some(laid) = &self.laid {
            if laid.failed {
                return Content::Message;
            }
            facts.widgets_empty = laid.widgets.is_empty();
        }
        model::decide(&self.setting, facts)
    }

    /// The picture that `content` draws, if the host holds it.
    pub(super) fn held_for(&self, content: Content) -> Option<Held> {
        let of = |kind: Kind| {
            self.pics
                .main
                .filter(|(held, _)| *held == kind)
                .map(|(_, held)| held)
        };
        match content {
            Content::Cat => of(Kind::Cat),
            Content::Message => of(Kind::Logo),
            Content::Picture => of(Kind::Picture),
            Content::Overlay => of(Kind::Over),
            Content::Cover => self.pics.cover,
            _ => None,
        }
    }

    /// What shows: the content and the picture it draws.
    fn shows(&self) -> (Content, Option<ImageId>) {
        let content = self.content();
        (content, self.held_for(content).map(|held| held.id))
    }

    /// Where the picture over the page goes and how opaque it is drawn; none
    /// for a screen set up for a widget or not for the page.
    pub(super) fn over_rect(
        &self,
        font: &Font<'_>,
        style: &Style,
        screen: Rect,
        held: Held,
    ) -> Result<Option<(Rect, u8)>, Error> {
        let Some(setup) = &self.over else {
            return Ok(None);
        };
        let measures = model::Screen {
            width: screen.width,
            height: screen.height,
            margin: style.margin,
            status: paperpaw_ui_kit::geometry::line_px(font, style.small)?
                .saturating_add(style.gap),
        };
        let (box_w, box_h) = model::box_size(measures, setup.percent);
        let size = |side: u32| u16::try_from(side).unwrap_or(u16::MAX);
        let (w, h) = model::fit((size(held.width), size(held.height)), box_w, box_h);
        let (x, y, w, h) = model::placed(measures, w, h, setup.percent == 100, &setup.position);
        Ok(Some((Rect::new(x, y, w, h), setup.opacity)))
    }

    /// Decides what shows and asks for its picture: called when the screen
    /// opens. Anything the host cannot do leaves the message; only a memory
    /// shortage is an error.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn begin(&mut self, host: &Host<'_>, cookie: &mut u32) -> Result<(), Error> {
        only_memory(self.advance(host, cookie))?;
        self.note_over(host)
    }

    /// The host's answer to a picture asked for: true if it was ours. The
    /// screen is refreshed when what shows changed.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn answer(
        &mut self,
        host: &Host<'_>,
        ready: &ImageReady,
        cookie: &mut u32,
    ) -> Result<bool, Error> {
        let Some((asked, kind)) = self.pics.asked else {
            return Ok(false);
        };
        if asked != ready.cookie() {
            return Ok(false);
        }
        self.pics.asked = None;
        let before = self.shows();
        let status = match (ready.status(), ready.image()) {
            (ImageStatus::Ready, Some(id)) => {
                let held = Held {
                    id,
                    width: ready.width(),
                    height: ready.height(),
                };
                self.pics.main = Some((kind, held));
                ImageStatus::Ready
            }
            (ImageStatus::Ready, None) => ImageStatus::Damaged,
            (status, _) => status,
        };
        if status != ImageStatus::Ready {
            self.fail(kind);
        }
        if kind == Kind::Over {
            let why = Failure::of(&self.pics.over_path, status)?;
            self.note(why);
        }
        only_memory(self.advance(host, cookie))?;
        self.note_over(host)?;
        if self.pics.asked.is_none() && self.shows() != before {
            let mut scratch = [0; 64];
            only_memory(self.refresh(&mut CommandSender::new(host, &mut scratch)))?;
        }
        Ok(true)
    }

    /// Gives the pictures back: the screen closes.
    pub fn release(&mut self, host: &Host<'_>) {
        let held = [
            self.pics.main.take().map(|(_, held)| held),
            self.pics.cover.take(),
        ];
        for held in held.into_iter().flatten() {
            let mut scratch = [0; 64];
            // A refusal leaves it to the host, which ends it with the plugin.
            CommandSender::new(host, &mut scratch)
                .image_release(held.id)
                .ok();
        }
        self.pics.asked = None;
    }

    /// A file failed: what it was for is not to be had.
    fn fail(&mut self, kind: Kind) {
        match kind {
            Kind::Picture => self.facts.picture = false,
            Kind::Over => self.facts.over_page = false,
            Kind::Cat | Kind::Logo => self.pics.given_up = true,
        }
    }

    /// Asks for the picture of what shows, or holds the cover; where there is
    /// none, the fact is cleared and what shows is decided again.
    fn advance(&mut self, host: &Host<'_>, cookie: &mut u32) -> Result<(), Error> {
        for _ in 0..MOST_STEPS {
            if self.pics.asked.is_some() {
                return Ok(());
            }
            let content = self.content();
            // The cover is the spare of a picture over the page.
            if content == Content::Overlay && !self.pics.cover_tried {
                self.find_cover(host)?;
            }
            let asked = match content {
                Content::Cover => {
                    self.find_cover(host)?;
                    if self.pics.cover.is_some() {
                        return Ok(());
                    }
                    continue;
                }
                Content::Cat | Content::Message if self.pics.given_up => return Ok(()),
                Content::Cat if self.pics.main.is_none() => Some((Kind::Cat, CAT_FILE)),
                Content::Message if self.pics.main.is_none() => Some((Kind::Logo, LOGO_FILE)),
                Content::Picture if self.pics.main.is_none() => Some((Kind::Picture, "")),
                Content::Overlay
                    if self.pics.main.is_none()
                        && self
                            .over
                            .as_ref()
                            .is_some_and(|setup| setup.widget.trim().is_empty()) =>
                {
                    Some((Kind::Over, ""))
                }
                _ => return Ok(()),
            };
            let Some((kind, file)) = asked else {
                return Ok(());
            };
            if !self.request(host, cookie, kind, file)? {
                self.fail(kind);
            }
        }
        Ok(())
    }

    /// Asks the host to prepare the file of `kind` (`file` is the embedded
    /// one's name in the data folder; empty for a file of the card, found
    /// here). False where it could not be asked for.
    fn request(
        &mut self,
        host: &Host<'_>,
        cookie: &mut u32,
        kind: Kind,
        file: &str,
    ) -> Result<bool, Error> {
        let (root, path) = match kind {
            Kind::Cat | Kind::Logo => {
                let bytes = if kind == Kind::Cat {
                    crate::cat::PICTURE
                } else {
                    LOGO
                };
                if !only_memory_ok(ensure(host, file, bytes))? {
                    return Ok(false);
                }
                (PrepareRoot::Data, String::from(file))
            }
            _ => {
                let chosen = self
                    .over
                    .as_ref()
                    .map(|setup| setup.picture.trim().trim_matches('/'))
                    .filter(|chosen| kind == Kind::Over && !chosen.is_empty());
                let path = match chosen {
                    Some(chosen) => Some(paperpaw_ui_kit::text::copy(chosen)?),
                    None => next_in_folder(host)?,
                };
                let Some(path) = path else {
                    if kind == Kind::Over {
                        self.note(Some(Failure::EmptyFolder));
                    }
                    return Ok(false);
                };
                (PrepareRoot::Library, path)
            }
        };
        let side = self.side_of(host, kind)?;
        *cookie = cookie.wrapping_add(1).max(1);
        let mut scratch = [0; 640];
        let request = PrepareImage::new(root, &path, *cookie).side(side);
        match CommandSender::new(host, &mut scratch).image_prepare(&request) {
            Ok(()) => {
                self.pics.asked = Some((*cookie, kind));
                if kind == Kind::Over {
                    self.pics.over_path = path;
                }
                Ok(true)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Not to be had (no consent for the card, a path refused).
            Err(_) => {
                if kind == Kind::Over {
                    self.note(Some(Failure::Missing(path)));
                }
                Ok(false)
            }
        }
    }

    /// The longest side the host decodes `kind` to (0: the screen's).
    fn side_of(&self, host: &Host<'_>, kind: Kind) -> Result<u32, Error> {
        if kind == Kind::Picture {
            return Ok(0);
        }
        let display = capture_snapshot(host)?;
        let (width, height) = (display.display_width(), display.display_height());
        let least = u32::from(width.min(height));
        Ok(match kind {
            Kind::Logo => least / 3,
            Kind::Over => {
                let percent = self.over.as_ref().map_or(100, |setup| setup.percent);
                let measures = model::Screen {
                    width,
                    height,
                    margin: 0,
                    status: 0,
                };
                let (box_w, box_h) = model::box_size(measures, percent);
                u32::from(box_w.max(box_h))
            }
            // Inside the margins of the interface, as drawn.
            _ => {
                let font = Font::acquire_role(host, FontRole::Ui)?;
                let margin = Style::read(host, &display, &font)?.margin;
                least.saturating_sub(u32::from(margin) * 2)
            }
        }
        .max(1))
    }

    /// Looks for the cover of the book read last, once.
    fn find_cover(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.pics.cover_tried {
            return Ok(());
        }
        self.pics.cover_tried = true;
        let display = capture_snapshot(host)?;
        let long = u32::from(display.display_width().max(display.display_height()));
        self.pics.cover = last_cover(host, long)?;
        self.facts.cover = self.pics.cover.is_some();
        Ok(())
    }
}

/// The room a cat, a cover or a picture is drawn in: inside the margins for
/// the cat, else the whole screen.
pub(super) fn room(content: Content, screen: Rect, margin: u16) -> Rect {
    if content != Content::Cat {
        return screen;
    }
    Rect::new(
        margin,
        margin,
        screen.width.saturating_sub(margin.saturating_mul(2)),
        screen.height.saturating_sub(margin.saturating_mul(2)),
    )
}

/// `held` as large as fits in `room`, in its middle, its proportions kept.
pub(super) fn centred(held: Held, room: Rect) -> Rect {
    let size = |side: u32| u16::try_from(side).unwrap_or(u16::MAX);
    let (w, h) = model::fit(
        (size(held.width), size(held.height)),
        room.width,
        room.height,
    );
    Rect::new(
        room.x + room.width.saturating_sub(w) / 2,
        room.y + room.height.saturating_sub(h) / 2,
        w,
        h,
    )
}

/// An error but a memory shortage is left to the screen: it shows the message.
fn only_memory(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Whether `result` is `Ok`, a memory shortage passed on.
fn only_memory_ok(result: Result<(), Error>) -> Result<bool, Error> {
    match result {
        Ok(()) => Ok(true),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(false),
    }
}

/// `bytes` as the file `name` of the data folder, written once: a file of the
/// size it has is the one written before.
fn ensure(host: &Host<'_>, name: &str, bytes: &[u8]) -> Result<(), Error> {
    if storage::size(host, Root::Data, name).is_ok_and(|size| size == bytes.len() as u64) {
        return Ok(());
    }
    DataStorage::new(host)
        .write(name, bytes)
        .map_err(|_| Error::Io)
}

/// The cover of the book opened last, as an image of the host's, made as
/// large as the screen's long side; none for no book read, none with a cover,
/// or a cover not made yet (the cat shows then).
fn last_cover(host: &Host<'_>, side: u32) -> Result<Option<Held>, Error> {
    let library = match Library::open(host) {
        Ok(library) => library,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(None),
    };
    let found = cover_of_last(host, &library, side);
    // Released whatever the query did; a refusal is left to the host.
    library.release(host).ok();
    found
}

fn cover_of_last(host: &Host<'_>, library: &Library, side: u32) -> Result<Option<Held>, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let query = Query::new()
        .sort(Sort::RecentlyOpened)
        .descending(true)
        .exclude_archive(false);
    let (key, generation) = {
        let page = match library.query(host, &query, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        };
        let generation = page.end().generation();
        let Some(Ok(row)) = page.rows().next() else {
            return Ok(None);
        };
        // The books opened come first: none is a library nothing was read in.
        if !row.opened() {
            return Ok(None);
        }
        (row.key(), generation)
    };
    let Ok(book) = BookRef::catalog(key, generation) else {
        return Ok(None);
    };
    match library.cover(host, book, side, &mut input, &mut out) {
        Ok(cover) => Ok(Some(Held {
            id: cover.image(),
            width: cover.info().width(),
            height: cover.info().height(),
        })),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        // No cover, or one still being made.
        Err(_) => Ok(None),
    }
}

/// The path of the next picture of the folder (`sleep.folder`), one after the
/// other from where the last sleep left off, and the count kept for the next
/// time (`next.txt`). None for a folder with no picture, one that is not
/// there or one this plugin may not read.
fn next_in_folder(host: &Host<'_>) -> Result<Option<String>, Error> {
    let folder = super::read_text(host, "sleep.folder")?;
    let folder = folder.trim().trim_matches('/');
    let pictures = list_folder(host, folder)?;
    let mut kept = [0; 32];
    let read = storage::read_at(host, Root::Data, NEXT, 0, &mut kept).unwrap_or(0);
    let shown = model::shown_before(kept.get(..read).unwrap_or(&[]));
    let Some((name, count)) = pictures.next(shown) else {
        return Ok(None);
    };
    let mut path = String::new();
    path.try_reserve_exact(folder.len() + 1 + name.len())
        .map_err(|_| Error::NoMemory)?;
    if !folder.is_empty() {
        path.push_str(folder);
        path.push('/');
    }
    path.push_str(name);
    // A failed write is not told: the same picture comes again.
    storage::write(
        host,
        Root::Data,
        NEXT,
        decimal(count, &mut [0; 20]).as_bytes(),
    )
    .ok();
    Ok(Some(path))
}

/// `value` in decimal, in `digits`.
fn decimal(mut value: usize, digits: &mut [u8; 20]) -> &str {
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    core::str::from_utf8(&digits[at..]).unwrap_or("0")
}

/// The pictures of `folder` on the card (the first of them by name).
fn list_folder(host: &Host<'_>, folder: &str) -> Result<Pictures, Error> {
    let files = match Files::open(host) {
        Ok(files) => files,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(Pictures::default()),
    };
    let found = read_folder(host, &files, folder);
    let released = files.release(host);
    let found = found?;
    released.ok();
    Ok(found)
}

fn read_folder(host: &Host<'_>, files: &Files, folder: &str) -> Result<Pictures, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut kept = Pictures::default();
    let mut cursor = 0;
    for _ in 0..MOST_PAGES {
        let page = match files.list(host, Root::Files, folder, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // No folder, a file, no consent for the card: no pictures.
            Err(_) => break,
        };
        for entry in page.entries() {
            kept.take(entry.name(), entry.is_folder())?;
        }
        match page.next() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(kept)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "fixture failures fail the test")]
mod tests {
    use super::*;

    fn held(width: u32, height: u32) -> Held {
        Held {
            id: ImageId::new(1).unwrap(),
            width,
            height,
        }
    }

    #[test]
    fn the_count_is_written_in_decimal() {
        assert_eq!(decimal(0, &mut [0; 20]), "0");
        assert_eq!(decimal(7, &mut [0; 20]), "7");
        assert_eq!(decimal(1_203, &mut [0; 20]), "1203");
        assert_eq!(decimal(usize::MAX, &mut [0; 20]), "18446744073709551615");
    }

    #[test]
    fn a_picture_is_fitted_in_the_middle_of_its_room() {
        let screen = Rect::new(0, 0, 480, 800);
        // The cat (760 x 501) as wide as the room inside the margins leaves.
        let cat = centred(held(760, 501), room(Content::Cat, screen, 20));
        assert_eq!(cat, Rect::new(20, 20 + (760 - 290) / 2, 440, 290));
        // A cover is as wide as the screen unless it is taller (then as high).
        assert_eq!(centred(held(600, 800), screen), Rect::new(0, 80, 480, 640));
        assert_eq!(
            centred(held(600, 800), room(Content::Cover, screen, 20)).x,
            0
        );
        assert_eq!(centred(held(800, 400), screen), Rect::new(0, 280, 480, 240));
        // A picture of no size is nowhere.
        assert_eq!(centred(held(0, 0), screen).width, 0);
    }

    #[test]
    fn the_embedded_files_are_png_the_host_decodes() {
        for png in [crate::cat::PICTURE, LOGO] {
            assert_eq!(png.get(..8), Some(&b"\x89PNG\r\n\x1a\n"[..]));
            // Not interlaced (byte 12 of the header chunk's body).
            assert_eq!(png.get(28), Some(&0));
        }
    }
}
