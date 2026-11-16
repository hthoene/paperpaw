//! The books the continue card and the recent shelf show: the books opened
//! last, from the library service (the catalog follows the reading events, so any
//! reader plugin works), and a book's cover as a picture the host draws.
use alloc::{string::String, vec::Vec};
use core::cell::{Cell, RefCell};
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

/// What the card, the shelf and the page's check share (one for each page of
/// the home screen, and one for the widgets other screens made): the books the
/// library listed, and which of them the host found gone. Each of them used to ask
/// the library itself, and a query is a score of card reads on the X4 (#267).
#[derive(Debug, Default)]
pub struct Listing {
    /// The books the library listed (the opened ones, at most one page) at the
    /// last read: a frame is drawn in bands, and every band asked the host
    /// again. Kept for the bands of one frame ([`Known::begin_draw`]) and let go
    /// by every call into the plugin that is no draw ([`Listing::fresh`]): a
    /// draw after the library changed asks again, as it always did.
    page: RefCell<Option<Vec<Book>>>,
    /// Whether a draw began since `page` was read by something that is no draw
    /// (what a measure or the page's check just read is used by the frame).
    by_draw: Cell<bool>,
    /// A draw is under way (its reads are the frame's).
    drawing: Cell<bool>,
    /// The frames begun, to let the instances of one frame agree on who begins it.
    frame: Cell<u32>,
    /// What the host said of each book's file (book, title, gone): a book that
    /// is there stays so across the catalog's generations (it changes with every
    /// book read, and the host's look is 17 reads of the card each time), one
    /// that is gone is asked again with the next generation.
    held: RefCell<Vec<Verdict>>,
}

#[derive(Debug, Clone, Copy)]
struct Verdict {
    generation: u32,
    key: u32,
    title: u64,
    gone: bool,
}

impl Listing {
    fn gone(&self, book: &Book) -> Option<bool> {
        let title = title_hash(&book.title);
        self.held
            .borrow()
            .iter()
            .find(|v| {
                v.key == book.key && v.title == title && (!v.gone || v.generation == book.generation)
            })
            .map(|v| v.gone)
    }

    fn remember(&self, book: &Book, gone: bool) -> Result<(), Error> {
        let title = title_hash(&book.title);
        let mut held = self.held.borrow_mut();
        held.retain(|v| !(v.key == book.key && v.title == title));
        // At most the rows one page holds: the cache never grows past what a read asks.
        if held.len() >= PAGE {
            held.remove(0);
        }
        mem::push(
            &mut held,
            Verdict {
                generation: book.generation,
                key: book.key,
                title,
                gone,
            },
        )
    }

    /// What the host said of the files is not kept any more (the page was laid
    /// out again, or the books changed).
    pub fn forget(&self) {
        self.held.borrow_mut().clear();
        self.fresh();
    }

    /// The list of books is asked of the host again at the next read: what
    /// happened since the last draw (an event, a tick, a tap) may have changed it.
    pub fn fresh(&self) {
        *self.page.borrow_mut() = None;
        self.by_draw.set(false);
    }

    /// A draw begins (`true`) or is over.
    pub fn drawing(&self, on: bool) {
        self.drawing.set(on);
    }

    /// An instance that drew before begins a frame again; `seen` is the frame
    /// it knew. The first of the frame lets the old list go, unless something
    /// that is no draw read it since.
    fn begin_frame(&self, seen: &Cell<u32>) {
        if seen.get() == self.frame.get() {
            if self.by_draw.get() {
                *self.page.borrow_mut() = None;
            }
            self.frame.set(self.frame.get().wrapping_add(1));
        }
        seen.set(self.frame.get());
    }

    /// The page, read once for as long as it is kept.
    fn page(&self, host: &Host<'_>) -> Result<(), Error> {
        if self.page.borrow().is_none() {
            let page = listed(host, PAGE)?;
            *self.page.borrow_mut() = Some(page);
            if !self.drawing.get() {
                self.by_draw.set(false);
            }
        }
        Ok(())
    }

    /// The books of the page (opened ones, files not looked at).
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn listed(&self, host: &Host<'_>) -> Result<usize, Error> {
        self.page(host)?;
        Ok(self.page.borrow().as_ref().map_or(0, Vec::len))
    }
}

/// What a read of the card or the shelf uses: its own instance's memory and
/// what its page shares.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    pub known: &'a Known,
    pub listing: &'a Listing,
}

/// What an instance of the card or the shelf keeps for itself.
#[derive(Debug, Default)]
pub struct Known {
    /// Where each book's cover stood when it was last asked (generation, key):
    /// a draw asks the host once, not once for each band of a frame.
    /// A cover that is ready is kept across generations (the key and the title
    /// tell the book): the catalog changes with every book read, and the
    /// host's look at the book's files costs more than a half second on the X4.
    covers: RefCell<Vec<(u32, u32, u64, Covered)>>,
    /// The `clip_top` of the last draw of this widget: bands of a frame come
    /// top down, so a draw not below the last one begins a frame.
    last_top: Cell<i32>,
    /// Whether this instance drew since the list was let go.
    by_draw: Cell<bool>,
    /// The frame of the [`Listing`] this instance knew at its last draw.
    seen: Cell<u32>,
}

impl Known {
    /// What the host said of the covers is not kept, besides the ones that are
    /// ready (a draw that finds the host ended its picture forgets those, too).
    pub fn forget(&self, listing: &Listing) {
        // A cover that is ready is the book's still.
        self.covers
            .borrow_mut()
            .retain(|(.., covered)| matches!(covered, Covered::Ready(_)));
        listing.fresh();
        self.fresh();
    }

    /// This instance asks again at the next read: what happened since the last
    /// draw may have changed what it shows.
    pub fn fresh(&self) {
        self.last_top.set(i32::MAX);
        self.by_draw.set(false);
    }

    /// A draw of this widget begins, its clip starting at row `top`: one that
    /// does not start below the last begins a frame, which asks the library
    /// again; the bands after it use what it read. What a measure just read
    /// (no draw between it and this one) is used by the frame.
    pub fn begin_draw(&self, top: i32, listing: &Listing) {
        if top <= self.last_top.get() && self.by_draw.get() {
            listing.begin_frame(&self.seen);
        }
        self.last_top.set(top);
        self.by_draw.set(true);
        listing.by_draw.set(true);
    }

    /// The covers are asked about anew at the next draw: one that was being
    /// made may be ready; the ready ones stay.
    pub fn forget_covers(&self) {
        self.covers
            .borrow_mut()
            .retain(|(.., covered)| matches!(covered, Covered::Ready(_)));
    }

    /// The covers are all asked about anew: the host ended a picture.
    pub fn forget_all_covers(&self) {
        self.covers.borrow_mut().clear();
    }

    fn cover_of(&self, book: &Book) -> Option<Covered> {
        let title = title_hash(&book.title);
        self.covers
            .borrow()
            .iter()
            .find(|(g, k, t, covered)| {
                *k == book.key
                    && (*g == book.generation
                        || (*t == title && matches!(covered, Covered::Ready(_))))
            })
            .map(|(.., covered)| *covered)
    }

    fn keep_cover(&self, book: &Book, covered: Covered) {
        let title = title_hash(&book.title);
        let mut covers = self.covers.borrow_mut();
        // Only what is ready outlives its generation, and a book has one place.
        covers.retain(|(g, k, t, old)| {
            (*g == book.generation || matches!(old, Covered::Ready(_)))
                && !(*k == book.key && *t == title)
        });
        // At most the rows one page holds; a refused allocation keeps nothing,
        // and the next draw asks again.
        if covers.capacity() == 0 && covers.try_reserve_exact(PAGE).is_err() {
            return;
        }
        if covers.len() >= PAGE {
            covers.remove(0);
        }
        covers.push((book.generation, book.key, title, covered));
    }
}

/// FNV-1a of a title, to tell a book of one generation from another's.
fn title_hash(title: &str) -> u64 {
    title.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The `take` books opened after the first `skip` whose files are on the card,
/// the one opened last first. A book never opened is not recent: the list ends
/// there. An empty or walking catalog has none yet. A book's file is looked up
/// by the host when it answers `library.book` (op 3), so a gone book costs one
/// call more; only as many books as are shown are asked about.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn read(
    host: &Host<'_>,
    listing: &Listing,
    skip: usize,
    take: usize,
) -> Result<Vec<Book>, Error> {
    recent(host, Some(listing), skip, take)
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
    listing: Option<&Listing>,
    skip: usize,
    take: usize,
) -> Result<Vec<Book>, Error> {
    let want = skip.saturating_add(take).min(PAGE);
    if want <= skip {
        return Ok(Vec::new());
    }
    let Some(listing) = listing else {
        // The tick's read: one query, never from what a draw kept.
        let mut books = listed(host, want)?;
        books.drain(..skip.min(books.len()));
        return Ok(books);
    };
    listing.page(host)?;
    let memo = listing.page.borrow();
    let all = memo.as_deref().unwrap_or_default();
    let mut books = Vec::new();
    let mut seen = 0;
    for book in all {
        if seen == want {
            break;
        }
        if gone_known(host, listing, book)? {
            continue;
        }
        seen += 1;
        if seen <= skip {
            continue;
        }
        books.try_reserve(1).map_err(|_| Error::NoMemory)?;
        books.push(Book {
            key: book.key,
            generation: book.generation,
            title: copy(&book.title)?,
            progress: book.progress,
        });
    }
    Ok(books)
}

/// The first `rows` books the catalog lists as opened, the one opened last
/// first. A book never opened is not recent: the list ends there.
fn listed(host: &Host<'_>, rows: usize) -> Result<Vec<Book>, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = (
            mem::buffer(mem::SERVICE_BUFFER)?,
            mem::buffer(mem::SERVICE_BUFFER)?,
        );
        let query = Query::new().sort(Sort::RecentlyOpened);
        let page = library.query(host, &query, &mut input, &mut out)?;
        let generation = page.end().generation();
        let mut books = Vec::new();
        for row in page.rows().take(PAGE) {
            let row = row?;
            if !row.opened() || books.len() == rows {
                break;
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

/// Whether the file of `book` is not on the card now, the library opened only
/// where the host has to be asked. A book the catalog no longer has (a newer
/// generation) is not asked about again: it is kept, as the query just listed it.
fn gone_known(host: &Host<'_>, listing: &Listing, book: &Book) -> Result<bool, Error> {
    if let Some(gone) = listing.gone(book) {
        return Ok(gone);
    }
    with_library(host, |library| {
        let (mut ask, mut answer) = (mem::buffer(1024)?, mem::buffer(mem::SERVICE_BUFFER)?);
        let reference = BookRef::catalog(book.key, book.generation)?;
        let gone = match library.book(host, reference, &mut ask[..], &mut answer[..]) {
            Ok(found) => found.missing(),
            Err(Error::NotFound) => true,
            Err(Error::State) => return Ok(false),
            Err(error) => return Err(error),
        };
        // A cache write that fails costs the next read one more call, not the draw.
        listing.remember(book, gone).ok();
        Ok(gone)
    })
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
    if let Some(covered) = known.cover_of(book) {
        return Ok(covered);
    }
    let covered = cover(host, library, book)?;
    known.keep_cover(book, covered);
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

    fn book(generation: u32, key: u32, title: &str) -> Book {
        Book {
            key,
            generation,
            title: String::from(title),
            progress: 0,
        }
    }

    fn ready() -> Covered {
        Covered::Ready(Cover {
            image: ImageId::new(5).unwrap(),
            width: 3,
            height: 4,
        })
    }

    #[test]
    fn a_cover_is_remembered_by_book_and_generation_until_forgotten() {
        let known = Known::default();
        let (a, b) = (book(1, 7, "A"), book(1, 8, "B"));
        assert_eq!(known.cover_of(&a), None);
        known.keep_cover(&a, Covered::Busy);
        known.keep_cover(&b, Covered::None);
        assert_eq!(known.cover_of(&a), Some(Covered::Busy));
        assert_eq!(known.cover_of(&b), Some(Covered::None));
        // Another generation of the catalog is another question.
        assert_eq!(known.cover_of(&book(2, 7, "A")), None);
        known.forget_covers();
        assert_eq!(known.cover_of(&a), None);
        // What the tick found changed forgets what was not ready with the files.
        known.keep_cover(&a, Covered::None);
        known.forget(&Listing::default());
        assert_eq!(known.cover_of(&a), None);
    }

    #[test]
    fn a_ready_cover_outlives_the_generation_and_the_tick_for_the_same_book() {
        let known = Known::default();
        known.keep_cover(&book(1, 7, "A"), ready());
        known.forget(&Listing::default());
        assert_eq!(known.cover_of(&book(2, 7, "A")), Some(ready()));
        // Another title at the key is another book.
        assert_eq!(known.cover_of(&book(2, 7, "B")), None);
        // A cover that is ready stays when covers that were being made are asked
        // about again; the host ending a picture lets it go.
        known.forget_covers();
        assert_eq!(known.cover_of(&book(2, 7, "A")), Some(ready()));
        known.forget_all_covers();
        assert_eq!(known.cover_of(&book(2, 7, "A")), None);
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
    fn a_book_that_is_there_stays_so_across_generations_and_one_gone_is_asked_again() {
        let listing = Listing::default();
        let (there, gone) = (book(1, 7, "A"), book(1, 8, "B"));
        assert_eq!(listing.gone(&there), None);
        listing.remember(&there, false).unwrap();
        listing.remember(&gone, true).unwrap();
        assert_eq!(listing.gone(&there), Some(false));
        assert_eq!(listing.gone(&gone), Some(true));
        // The next generation asks anew about the one gone only; another title
        // at the key is another book.
        assert_eq!(listing.gone(&book(2, 7, "A")), Some(false));
        assert_eq!(listing.gone(&book(2, 8, "B")), None);
        assert_eq!(listing.gone(&book(2, 7, "C")), None);
        listing.forget();
        assert_eq!(listing.gone(&there), None);
    }

    #[test]
    fn the_cache_holds_no_more_than_a_page_of_books() {
        let listing = Listing::default();
        for key in 0..(PAGE as u32 + 5) {
            listing.remember(&book(1, key, "A"), false).unwrap();
        }
        assert_eq!(listing.held.borrow().len(), PAGE);
        assert_eq!(listing.gone(&book(1, PAGE as u32 + 1, "A")), Some(false));
        assert_eq!(listing.gone(&book(1, 0, "A")), None);
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
