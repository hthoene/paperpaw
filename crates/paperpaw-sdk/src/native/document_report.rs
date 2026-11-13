//! Where the reader is (`docs/abi/document.md` 2.4, op 22): the reader tells the host, and the
//! host turns it into the book events (`event.book_opened`, `page_turned`, `book_closed`,
//! `book_finished`), so no plugin raises them itself.
use super::*;

/// Where the user is reading, to report with [`Documents::report`]. Built from the place and
/// the progress; every other part is optional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Report<'a> {
    at: DocPosition,
    insert: u32,
    progress_ppm: u32,
    finished: bool,
    title: &'a str,
    chapter: &'a str,
    pub(super) page: Option<u32>,
    pub(super) pages: Option<u32>,
    chapter_left: Option<u32>,
}

impl<'a> Report<'a> {
    /// A page that starts at `at`, `progress_ppm` millionths into the book (at most 1,000,000).
    #[must_use]
    pub const fn new(at: DocPosition, progress_ppm: u32) -> Self {
        Self {
            at,
            insert: 0,
            progress_ppm,
            finished: false,
            title: "",
            chapter: "",
            page: None,
            pages: None,
            chapter_left: None,
        }
    }
    /// The insert ordinal at the page's start (zero is source text).
    #[must_use]
    pub const fn insert(mut self, insert: u32) -> Self {
        self.insert = insert;
        self
    }
    /// The user reached the end of the book (raises `book_finished` once per session).
    #[must_use]
    pub const fn finished(mut self) -> Self {
        self.finished = true;
        self
    }
    /// The book's display title, at most 256 bytes (cut at a character boundary by the reader).
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }
    /// The title of the table-of-contents entry the page is in, at most 256 bytes; empty where
    /// the book has none or the page comes before its first entry.
    #[must_use]
    pub const fn chapter(mut self, chapter: &'a str) -> Self {
        self.chapter = chapter;
        self
    }
    /// This page of `pages`, counted from 1 (both, or neither).
    #[must_use]
    pub const fn pages(mut self, page: u32, pages: u32) -> Self {
        self.page = Some(page);
        self.pages = Some(pages);
        self
    }
    /// The pages after this one in its chapter, 0 on its last page. Only with
    /// [`pages`](Self::pages) and a non-empty [`chapter`](Self::chapter), and at most the pages
    /// after this one.
    #[must_use]
    pub const fn chapter_left(mut self, left: u32) -> Self {
        self.chapter_left = Some(left);
        self
    }
    /// Refuses what the host would refuse (`document.md` 2.4), before anything is sent.
    fn check(&self) -> Result<(), Error> {
        if self.progress_ppm > PROGRESS_MAX {
            return Err(Error::Invalid);
        }
        // Pages left in a chapter need a chapter the reader knows the end of.
        if self.chapter_left.is_some() && self.chapter.is_empty() {
            return Err(Error::Invalid);
        }
        match (self.page, self.pages) {
            (None, None) => {
                if self.chapter_left.is_some() {
                    return Err(Error::Invalid);
                }
            }
            (Some(page), Some(pages)) => {
                if page == 0 || page > pages {
                    return Err(Error::Invalid);
                }
                if self.chapter_left.is_some_and(|left| left > pages - page) {
                    return Err(Error::Invalid);
                }
            }
            _ => return Err(Error::Invalid),
        }
        if self.title.len() > wire::SERVICE_DOC_REPORT_TITLE_MAX_BYTES
            || self.chapter.len() > wire::SERVICE_DOC_REPORT_CHAPTER_MAX_BYTES
        {
            return Err(Error::NoSpace);
        }
        Ok(())
    }
}

/// `progress_ppm` at the end of the book.
const PROGRESS_MAX: u32 = 1_000_000;
/// `doc_report.flags` bit 0: the user reached the end of the book.
const FINISHED: u32 = 1;

impl Documents {
    /// Tell the host where the user is reading `document` (op 22). The first report of a book
    /// starts the reading session (`event.book_opened`), a later one that differs raises
    /// `event.page_turned`, and one with [`Report::finished`] raises `event.book_finished`
    /// once; an identical report raises nothing, so repeating it is free. A document that is
    /// not a book of the library (root 0 or 5, an index build), and a report from a plugin that
    /// does not own the top reading-app instance, are accepted and raise nothing, so a reader
    /// reports every document the same way. `input` of 1,024 bytes and `out` of
    /// 64 always suffice.
    ///
    /// # Errors
    /// `Invalid` for what the host would refuse (progress above 1,000,000; a page without its
    /// count or the reverse, a page of 0 or past the count; chapter pages left without the
    /// pages, without a chapter, or past the pages after this one: nothing is sent) or for a reply that is not
    /// empty; `NoSpace` for a title or chapter over 256 bytes (never cut); the host's
    /// refusals: `Invalid` for a section past the document's, `Denied` for another plugin's
    /// document, `Cancelled` for a closed one, `Utf8` for text that is not UTF-8.
    pub fn report(
        &self,
        host: &Host<'_>,
        document: DocId,
        report: &Report<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        report.check()?;
        let record = wire::ServiceDocReport {
            document: document.0,
            section: report.at.section,
            offset: report.at.offset,
            insert: report.insert,
            progress_ppm: report.progress_ppm,
            flags: if report.finished { FINISHED } else { 0 },
            title: report.title,
            chapter: report.chapter,
            page: report.page,
            pages: report.pages,
            chapter_left: report.chapter_left,
        };
        if self.call(host, schema::OP_REPORT, &[&record], input, out)? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
