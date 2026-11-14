//! The books the continue card and the recent shelf show: the books opened
//! last, from the library service (the catalog follows the reading events, so any
//! reader plugin works), and a book's cover as a picture the host draws.
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        image::ImageId,
        library::{BookRef, Library, Query, Sort},
    },
};
use paperpaw_ui_kit::{mem, text::copy};

/// Covers a shelf holds at most (and the setting's largest count).
pub const MOST: usize = 8;

/// The rows one library page holds at most; the shelf and the card read one
/// page, so the current book and the shelf's books are among them.
const PAGE: usize = 10;

/// A recent book: where it is in the catalog, its title and how far it is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    pub key: u32,
    pub generation: u32,
    pub title: String,
    /// Percent, 0 to 100.
    pub progress: u8,
}

/// Runs `work` with the library service open and releases it whatever `work`
/// ends in; a failure of `work` is reported before one of the release.
///
/// # Errors
/// The host's refusals, whatever `work` returns.
pub fn with_library<T>(
    host: &Host<'_>,
    work: impl FnOnce(&Library) -> Result<T, Error>,
) -> Result<T, Error> {
    let library = Library::open(host)?;
    let done = work(&library);
    let released = library.release(host);
    let done = done?;
    released?;
    Ok(done)
}

/// Which books of a catalog generation the host found gone, kept by an instance
/// so that a redraw or a tap does not ask the card again: asked once per book
/// and generation (the host looks at the card for each `library.book`), and
/// again when the generation changes or the instance forgets (the tick found
/// what it shows changed).
#[derive(Debug, Default)]
pub struct Known {
    held: RefCell<(u32, Vec<(u32, bool)>)>,
    /// Where each book's cover stood when it was last asked (generation, key):
    /// a draw asks the host once, not once for each band of a frame.
    covers: RefCell<Vec<(u32, u32, Covered)>>,
}

impl Known {
    fn gone(&self, generation: u32, key: u32) -> Option<bool> {
        let held = self.held.borrow();
        if held.0 != generation {
            return None;
        }
        held.1
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, gone)| *gone)
    }

    fn remember(&self, generation: u32, key: u32, gone: bool) -> Result<(), Error> {
        let mut held = self.held.borrow_mut();
        if held.0 != generation {
            *held = (generation, Vec::new());
        }
        // At most the rows one page holds: the cache never grows past what a read asks.
        if held.1.len() >= PAGE {
            return Ok(());
        }
        mem::push(&mut held.1, (key, gone))
    }

    /// What the host said is not kept any more.
    pub fn forget(&self) {
        *self.held.borrow_mut() = (0, Vec::new());
        self.forget_covers();
    }

    /// The covers are asked about anew at the next draw: one that was being
    /// made may be ready, one the host ended is made again.
    pub fn forget_covers(&self) {
        self.covers.borrow_mut().clear();
    }

    fn cover_of(&self, generation: u32, key: u32) -> Option<Covered> {
        self.covers
            .borrow()
            .iter()
            .find(|(g, k, _)| *g == generation && *k == key)
            .map(|(.., covered)| *covered)
    }

    fn keep_cover(&self, generation: u32, key: u32, covered: Covered) {
        let mut covers = self.covers.borrow_mut();
        // At most the rows one page holds; a refused allocation keeps nothing,
        // and the next draw asks again.
        if covers.len() < PAGE && covers.try_reserve(1).is_ok() {
            covers.push((generation, key, covered));
        }
    }
}

/// The `take` books opened after the first `skip` whose files are on the card,
/// the one opened last first. A book never opened is not recent: the list ends
/// there. An empty or walking catalog has none yet. A book's file is looked up
/// by the host when it answers `library.book` (op 3), so a gone book costs one
/// call more; only as many books as are shown are asked about.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn read(host: &Host<'_>, known: &Known, skip: usize, take: usize) -> Result<Vec<Book>, Error> {
    recent(host, Some(known), skip, take)
}

/// [`read`] without asking about the files: the books the catalog lists, for
/// telling at each tick whether what a widget shows changed (the draw asks
/// about the files, so the tick's library cost stays one query).
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn read_listed(host: &Host<'_>, skip: usize, take: usize) -> Result<Vec<Book>, Error> {
    recent(host, None, skip, take)
}

fn recent(
    host: &Host<'_>,
    known: Option<&Known>,
    skip: usize,
    take: usize,
) -> Result<Vec<Book>, Error> {
    let want = skip.saturating_add(take).min(PAGE);
    if want <= skip {
        return Ok(Vec::new());
    }
    with_library(host, |library| {
        let (mut input, mut out) = (
            mem::buffer(mem::SERVICE_BUFFER)?,
            mem::buffer(mem::SERVICE_BUFFER)?,
        );
        let query = Query::new().sort(Sort::RecentlyOpened);
        let page = library.query(host, &query, &mut input, &mut out)?;
        let generation = page.end().generation();
        // The book calls need buffers of their own; the tick's read has none.
        let mut asking = match known {
            Some(_) => Some((mem::buffer(1024)?, mem::buffer(mem::SERVICE_BUFFER)?)),
            None => None,
        };
        let mut books = Vec::new();
        let mut seen = 0;
        for row in page.rows().take(PAGE) {
            let row = row?;
            if !row.opened() || seen == want {
                break;
            }
            if let (Some(known), Some((ask, answer))) = (known, asking.as_mut()) {
                let (key, buffers) = (row.key(), (&mut ask[..], &mut answer[..]));
                if gone(host, library, (known, generation, key), buffers)? {
                    continue;
                }
            }
            seen += 1;
            if seen <= skip {
                continue;
            }
            books.try_reserve(1).map_err(|_| Error::NoMemory)?;
            books.push(Book {
                key: row.key(),
                generation,
                title: copy(row.title())?,
                progress: row.progress().unwrap_or(0),
            });
        }
        Ok(books)
    })
}

/// Whether the file of the book `key` is not on the card now. A book the
/// catalog no longer has (a newer generation) is not asked about again: it is
/// kept, as the query just listed it.
fn gone(
    host: &Host<'_>,
    library: &Library,
    (known, generation, key): (&Known, u32, u32),
    (ask, answer): (&mut [u8], &mut [u8]),
) -> Result<bool, Error> {
    if let Some(gone) = known.gone(generation, key) {
        return Ok(gone);
    }
    let reference = BookRef::catalog(key, generation)?;
    let gone = match library.book(host, reference, ask, answer) {
        Ok(book) => book.missing(),
        Err(Error::NotFound) => true,
        Err(Error::State) => return Ok(false),
        Err(error) => return Err(error),
    };
    // A cache write that fails costs the next read one more call, not the draw.
    known.remember(generation, key, gone).ok();
    Ok(gone)
}

/// What `books` show, to tell when it changed: their keys, titles and how far
/// each is read (FNV-1a).
#[must_use]
pub fn signature(books: &[Book]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut feed = |byte: u8| {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    };
    for book in books {
        for byte in book.key.to_le_bytes() {
            feed(byte);
        }
        for byte in book.title.bytes() {
            feed(byte);
        }
        feed(book.progress);
        feed(0xff);
    }
    hash
}

/// A cover the host has made, and how big it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cover {
    pub image: ImageId,
    pub width: u32,
    pub height: u32,
}

/// Where a book's cover stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Covered {
    /// Ready to draw.
    Ready(Cover),
    /// Being made: asked again at the next minute.
    Busy,
    /// None to show: the book has none, or the host draws no pictures.
    None,
}

/// `book`'s cover at the list's size (the host keeps it in its cover cache and
/// ends an image id it needs room for: asked again on each draw).
///
/// # Errors
/// [`Error::NoMemory`] and refusals of the host that are not about this book's
/// cover.
pub fn cover(host: &Host<'_>, library: &Library, book: &Book) -> Result<Covered, Error> {
    let (mut input, mut out) = (mem::buffer(1024)?, mem::buffer(mem::SERVICE_BUFFER)?);
    let reference = BookRef::catalog(book.key, book.generation)?;
    match library.cover(host, reference, 0, &mut input, &mut out) {
        Ok(cover) => Ok(Covered::Ready(Cover {
            image: cover.image(),
            width: cover.info().width(),
            height: cover.info().height(),
        })),
        Err(Error::Busy) => Ok(Covered::Busy),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        // No cover, a book gone from the catalog, a host that draws no pictures:
        // the book is shown without one.
        Err(_) => Ok(Covered::None),
    }
}

/// [`cover`], asked once for each book and generation until the instance
/// forgets (`Known::forget_covers`: the tick found the books changed or a cover
/// that was being made), so that the bands of a frame, drawn one after the
/// other, do not each make the host look at the book's files again.
///
/// # Errors
/// As [`cover`].
pub fn cover_known(
    host: &Host<'_>,
    known: &Known,
    library: &Library,
    book: &Book,
) -> Result<Covered, Error> {
    if let Some(covered) = known.cover_of(book.generation, book.key) {
        return Ok(covered);
    }
    let covered = cover(host, library, book)?;
    known.keep_cover(book.generation, book.key, covered);
    Ok(covered)
}

/// The pictures a draw puts on the canvas after its paint is replayed (a cover
/// goes where nothing else is drawn), and whether one is still being made.
#[derive(Debug, Default)]
pub struct Pictures {
    pub placed: Vec<(ImageId, Rect)>,
    pub busy: bool,
}

impl Pictures {
    /// `image` goes at `at`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn place(&mut self, image: ImageId, at: Rect) -> Result<(), Error> {
        mem::push(&mut self.placed, (image, at))
    }
}

/// `width` by `height` fitted into `place`, as large as fits and in the shape
/// it has; `x` and `y` say where in the place (0 left or top, 1 right or bottom).
#[must_use]
pub fn fitted(size: (u32, u32), place: Rect, x: f32, y: f32) -> Rect {
    let (width, height) = (u64::from(size.0.max(1)), u64::from(size.1.max(1)));
    let (room_w, room_h) = (u64::from(place.width), u64::from(place.height));
    let (w, h) = if width * room_h > height * room_w {
        (room_w, (height * room_w / width).max(1))
    } else {
        ((width * room_h / height).max(1), room_h)
    };
    let clamp = paperpaw_ui_kit::geometry::clamp_u16;
    let (w, h) = (
        clamp(w.min(room_w) as i64).min(place.width),
        clamp(h.min(room_h) as i64).min(place.height),
    );
    let left = f32::from(place.width - w) * x;
    let top = f32::from(place.height - h) * y;
    Rect::new(place.x + left as u16, place.y + top as u16, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_is_remembered_by_book_and_generation_until_forgotten() {
        let known = Known::default();
        assert_eq!(known.cover_of(1, 7), None);
        known.keep_cover(1, 7, Covered::Busy);
        known.keep_cover(1, 8, Covered::None);
        assert_eq!(known.cover_of(1, 7), Some(Covered::Busy));
        assert_eq!(known.cover_of(1, 8), Some(Covered::None));
        // Another generation of the catalog is another question.
        assert_eq!(known.cover_of(2, 7), None);
        known.forget_covers();
        assert_eq!(known.cover_of(1, 7), None);
        // What the tick found changed forgets the covers with the files.
        known.keep_cover(1, 7, Covered::None);
        known.forget();
        assert_eq!(known.cover_of(1, 7), None);
    }

    #[test]
    fn a_picture_is_fitted_in_its_shape_where_the_place_says() {
        let place = Rect::new(10, 20, 100, 100);
        // Twice as high as wide: as high as the place, half as wide.
        assert_eq!(
            fitted((50, 100), place, 0.0, 0.5),
            Rect::new(10, 20, 50, 100)
        );
        assert_eq!(
            fitted((50, 100), place, 0.5, 1.0),
            Rect::new(35, 20, 50, 100)
        );
        // Wider than high, at the bottom.
        assert_eq!(
            fitted((100, 50), place, 0.5, 1.0),
            Rect::new(10, 70, 100, 50)
        );
    }

    #[test]
    fn a_degenerate_place_or_picture_is_fitted_without_panic() {
        assert_eq!(fitted((0, 0), Rect::new(1, 2, 0, 0), 0.5, 0.5).width, 0);
        let _ = fitted(
            (u32::MAX, u32::MAX),
            Rect::new(0, 0, u16::MAX, u16::MAX),
            1.0,
            1.0,
        );
    }

    #[test]
    fn what_the_host_said_is_kept_for_its_generation_and_forgotten() {
        let known = Known::default();
        assert_eq!(known.gone(1, 7), None);
        known.remember(1, 7, true).unwrap();
        known.remember(1, 8, false).unwrap();
        assert_eq!(known.gone(1, 7), Some(true));
        assert_eq!(known.gone(1, 8), Some(false));
        // Another generation asks anew, and holds only its own.
        assert_eq!(known.gone(2, 7), None);
        known.remember(2, 9, true).unwrap();
        assert_eq!(known.gone(1, 7), None);
        assert_eq!(known.gone(2, 9), Some(true));
        known.forget();
        assert_eq!(known.gone(2, 9), None);
    }

    #[test]
    fn the_cache_holds_no_more_than_a_page_of_books() {
        let known = Known::default();
        for key in 0..(PAGE as u32 + 5) {
            known.remember(1, key, false).unwrap();
        }
        assert_eq!(known.held.borrow().1.len(), PAGE);
        assert_eq!(known.gone(1, PAGE as u32 + 1), None);
    }

    #[test]
    fn what_the_books_show_changes_the_signature() {
        let book = |progress| Book {
            key: 7,
            generation: 1,
            title: String::from("Title"),
            progress,
        };
        assert_eq!(signature(&[book(10)]), signature(&[book(10)]));
        assert_ne!(signature(&[book(10)]), signature(&[book(11)]));
        assert_ne!(signature(&[book(10)]), signature(&[]));
    }
}
