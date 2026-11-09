//! Paged list replies (sections, table of contents, links, fonts) and picture
//! replies, written row by row straight into the caller's output.
use super::{Error, FormatCall, FormatOp, schema};

/// Most faces a book's fonts list holds (`document.md` 6: 31 families times four).
const FONT_FACES: u32 = 124;
/// Highest table-of-contents level (`document.md` 3).
const TOC_LEVEL_MAX: u32 = 15;
/// `fmt.toc` flag: the title was cut at a UTF-8 boundary.
const TOC_TITLE_CUT: u32 = 2;
/// `fmt.end` flag: the last page.
const END_LAST: u32 = 1;
/// Bytes kept free after the rows for the closing `fmt.end` record: its framing, four
/// `u32` fields and the optional cursor, rounded up.
const END_RESERVE: usize = 32;
/// Framing of one row (tag, length, field bitmap), rounded up.
const ROW_FRAMING: usize = 5;

/// How a page of rows ends: the last page, or one the host continues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageEnd {
    last: bool,
    total: u32,
    next: u32,
    generation: u32,
}
impl PageEnd {
    /// The last page of a list of `total` rows (the whole list's, not this page's).
    /// `generation` is the format's own for this document, never 0 (`document.md`
    /// 3): the same value on every page of one list.
    #[must_use]
    pub const fn last(total: u32, generation: u32) -> Self {
        Self {
            last: true,
            total,
            next: 0,
            generation,
        }
    }
    /// A page the host continues: it asks again with `cursor` set to `next` and
    /// the `generation` given here, never 0. The provider checks it on that call
    /// (`FormatCall::generation`) and answers `State` for a stale one, so the
    /// host starts again at 0; a first call carries cursor 0 and generation 0.
    #[must_use]
    pub const fn more(total: u32, next: u32, generation: u32) -> Self {
        Self {
            last: false,
            total,
            next,
            generation,
        }
    }
}

/// The rows of one page reply, written as they come. A push that does not fit
/// leaves the earlier rows as they were: finish the page with
/// [`PageEnd::more`] and continue from there.
pub struct FormatRows<'w> {
    writer: schema::ReplyWriter<'w>,
    op: FormatOp,
    /// The most bytes the output may hold: its length, within the reply cap.
    cap: usize,
    /// An upper bound of the bytes the rows hold, the count byte included.
    used: usize,
}
impl core::fmt::Debug for FormatRows<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FormatRows")
            .field("op", &self.op)
            .field("rows", &self.writer.len())
            .finish_non_exhaustive()
    }
}

impl FormatCall<'_> {
    /// Begin the page reply of a list operation: sections, table of contents,
    /// links or fonts. Rows are written into `out`, which stays untouched only
    /// until the first row.
    ///
    /// # Errors
    /// State for an operation that is no list; NoSpace when `out` cannot hold even
    /// an empty page (33 bytes).
    pub fn rows<'w>(&self, out: &'w mut [u8]) -> Result<FormatRows<'w>, Error> {
        self.require(&[
            FormatOp::Sections,
            FormatOp::Toc,
            FormatOp::Links,
            FormatOp::Fonts,
        ])?;
        let cap = out.len().min(schema::MAX_REPLY);
        if cap < 1 + END_RESERVE {
            // Not even an empty page fits.
            return Err(Error::NoSpace);
        }
        let writer = schema::ReplyWriter::new(out).map_err(Error::from_status)?;
        Ok(FormatRows {
            writer,
            op: self.operation(),
            cap,
            used: 1,
        })
    }
    /// Reply with the picture a cover or image request names: its cache file in
    /// the document's cache (root 6), the format's own picture `index`
    /// (`u32::MAX` for the cover), its size, and whether it is decoded yet.
    ///
    /// # Errors
    /// State outside cover and image; Invalid for a path over 256 bytes or an
    /// empty one; NoSpace for a short output buffer.
    pub fn reply_image(
        &self,
        path: &str,
        index: u32,
        width: u32,
        height: u32,
        decoded: bool,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        self.require(&[FormatOp::Cover, FormatOp::Image])?;
        if path.is_empty() || path.len() > schema::FMT_IMAGE_PATH_MAX_BYTES {
            return Err(Error::Invalid);
        }
        schema::FmtImage::new(path, index, width, height, u32::from(!decoded))
            .encode(out)
            .map_err(Error::from_status)
    }
}

impl FormatRows<'_> {
    fn expect(&self, op: FormatOp) -> Result<(), Error> {
        if self.op == op {
            Ok(())
        } else {
            Err(Error::State)
        }
    }
    /// Write a row of at most `bound` bytes. The page always keeps room for its
    /// closing record: a row that would use it, or one past the row cap, is NoSpace.
    fn row(&mut self, record: &schema::Record<'_>, bound: usize) -> Result<(), Error> {
        let used = self.used + ROW_FRAMING + bound;
        if used + END_RESERVE > self.cap || self.writer.len() + 1 >= schema::REPLY_ROWS {
            return Err(Error::NoSpace);
        }
        self.writer.push(record).map_err(Error::from_status)?;
        self.used = used;
        Ok(())
    }
    /// Rows written so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.writer.len()
    }
    /// Whether no row is written yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.writer.is_empty()
    }
    /// A section: its `index`, its `length` in source units (what progress is
    /// computed from) and whether it is a whole-page picture.
    ///
    /// # Errors
    /// State when the request is not `sections`; NoSpace when the row does not
    /// fit or the page is full (end it with [`PageEnd::more`]).
    pub fn section(&mut self, index: u32, length: u32, whole_page: bool) -> Result<(), Error> {
        self.expect(FormatOp::Sections)?;
        self.row(
            &schema::Record::FmtSection(schema::FmtSection::new(
                index,
                length,
                u32::from(whole_page),
            )),
            12,
        )
    }
    /// A table-of-contents entry. `level` is 0 to 15; `approximate` says
    /// `toc_target` finds the exact place. A title over 512 bytes is cut at a
    /// character boundary and flagged.
    ///
    /// # Errors
    /// State when the request is not `toc`; Invalid for a level above 15;
    /// NoSpace when the row does not fit.
    pub fn toc(
        &mut self,
        index: u32,
        level: u32,
        section: u32,
        offset: u32,
        approximate: bool,
        title: &str,
    ) -> Result<(), Error> {
        self.expect(FormatOp::Toc)?;
        if level > TOC_LEVEL_MAX {
            return Err(Error::Invalid);
        }
        let shown = cut(title, schema::FMT_TOC_TITLE_MAX_BYTES);
        let flags = u32::from(approximate)
            | if shown.len() < title.len() {
                TOC_TITLE_CUT
            } else {
                0
            };
        self.row(
            &schema::Record::FmtToc(schema::FmtToc::new(
                index, level, section, offset, flags, shown,
            )),
            20 + 2 + shown.len(),
        )
    }
    /// A link overlapping the requested range: the source `start..end` it covers,
    /// its kind and its target. An `href` over 1,024 bytes is not listed
    /// (`document.md` 3): the row is skipped and `Ok(false)` says so.
    ///
    /// # Errors
    /// State when the request is not `links`; Invalid for a start after the end;
    /// NoSpace when the row does not fit.
    pub fn link(
        &mut self,
        start: u32,
        end: u32,
        kind: LinkKind,
        href: &str,
    ) -> Result<bool, Error> {
        self.expect(FormatOp::Links)?;
        if start > end {
            return Err(Error::Invalid);
        }
        if href.len() > schema::FMT_LINK_HREF_MAX_BYTES {
            return Ok(false);
        }
        self.row(
            &schema::Record::FmtLink(schema::FmtLink::new(start, end, kind as u32, href)),
            12 + 2 + href.len(),
        )?;
        Ok(true)
    }
    /// An embedded font face: `index` from 0 below 124, its `family` 1 to 31 as the
    /// runs' text style names it, bold and italic, and the file's size in bytes
    /// (written whole to `fonts/<index>` in the cache).
    ///
    /// # Errors
    /// State when the request is not `fonts`; Invalid for an index of 124 or more
    /// or a family outside 1 to 31; NoSpace when the row does not fit.
    pub fn font(
        &mut self,
        index: u32,
        family: u32,
        bold: bool,
        italic: bool,
        bytes: u32,
    ) -> Result<(), Error> {
        self.expect(FormatOp::Fonts)?;
        if index >= FONT_FACES || !(1..=31).contains(&family) {
            return Err(Error::Invalid);
        }
        self.row(
            &schema::Record::FmtFont(schema::FmtFont::new(
                index,
                family,
                u32::from(bold) | (u32::from(italic) << 1),
                bytes,
            )),
            16,
        )
    }
    /// End the page and return the reply's length in the output buffer.
    ///
    /// # Errors
    /// Invalid for generation 0. Rows always leave room for this record, so it is
    /// never short of space.
    pub fn finish(mut self, end: PageEnd) -> Result<usize, Error> {
        if end.generation == 0 {
            return Err(Error::Invalid);
        }
        self.writer
            .push(&schema::Record::FmtEnd(schema::FmtEnd::new(
                if end.last { END_LAST } else { 0 },
                end.total,
                end.generation,
                end.next,
                None,
            )))
            .map_err(Error::from_status)?;
        self.writer.finish().map_err(Error::from_status)
    }
}

/// What a link points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[repr(u32)]
pub enum LinkKind {
    /// A place in this book.
    Internal = 0,
    /// A web address or another book.
    External = 1,
    /// A footnote or note, shown at its target.
    Footnote = 2,
}

fn cut(text: &str, cap: usize) -> &str {
    let mut end = text.len().min(cap);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}
