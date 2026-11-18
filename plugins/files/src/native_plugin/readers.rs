//! Which reading app opens a book (#260), where two or more read it: *Open
//! with…* this time, *Always open with…* for the book and *For this folder*
//! for the books in its folder. The host keeps the choice when the book opens
//! with the hand-over param `keep` and resolves it at every open after.
use super::*;

/// Reading apps listed at most.
const MOST_READERS: usize = 12;
/// The reading apps' list, a page of it: the request.
const READERS_INPUT: usize = 256;
/// The hand-over param that keeps the reading app chosen (`book`, `folder`).
const KEEP: &str = "keep";

impl NativeFiles {
    /// The reading apps of books of `format` (all where it is empty): their
    /// screens and titles, at most [`MOST_READERS`]. None where the device
    /// has no document service.
    pub(super) fn readers_for(
        &mut self,
        host: &Host<'_>,
        format: &str,
    ) -> Result<Vec<(String, String)>, Error> {
        if self.documents.is_none() {
            match Documents::open(host) {
                Ok(documents) => self.documents = Some(documents),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(Vec::new()),
            }
        }
        let documents = self.documents.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(READERS_INPUT)?, buffer(SERVICE_BUFFER)?);
        let mut found: Vec<(String, String)> = Vec::new();
        let mut cursor = ReaderCursor::start();
        // The list may change between pages: start again once, then give up.
        let mut again = true;
        loop {
            let page = match documents.readers(host, format, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::State) if again => {
                    again = false;
                    found.clear();
                    cursor = ReaderCursor::start();
                    continue;
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(Vec::new()),
            };
            for reader in page.rows() {
                if found.len() >= MOST_READERS {
                    return Ok(found);
                }
                push(&mut found, (copy(reader.screen())?, copy(reader.title())?))?;
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => return Ok(found),
            }
        }
    }

    /// The format of the book at `path`, as the library knows it (empty where
    /// it does not).
    fn format_of(&self, host: &Host<'_>, path: &str) -> Result<String, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match BookRef::path(path).and_then(|book| library.book(host, book, &mut input, &mut out)) {
            Ok(book) => copy(book.format()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => copy(""),
        }
    }

    /// Asks which reading app opens the book at `path`; one that `keep`s the
    /// choice offers *Standard* first (the choice forgotten).
    pub(super) fn ask_reader(
        &mut self,
        host: &Host<'_>,
        path: &str,
        keep: Option<&'static str>,
    ) -> Result<(), Error> {
        let format = self.format_of(host, path)?;
        let readers = self.readers_for(host, &format)?;
        if readers.len() < 2 {
            return self.open_path(host, path);
        }
        let mut say = Messages::new()?;
        let mut words: Vec<String> = Vec::new();
        let mut screens: Vec<String> = Vec::new();
        if keep.is_some() {
            push(&mut words, say.copy(host, "files.reading-app.default")?)?;
            push(&mut screens, String::new())?;
        }
        for (screen, title) in readers {
            push(&mut words, title)?;
            push(&mut screens, screen)?;
        }
        let title = say.copy(
            host,
            match keep {
                None => "files.option.open-with",
                Some("book") => "files.option.always-with",
                Some(_) => "files.option.for-folder",
            },
        )?;
        let choices = crate::native_sheet::choices(&words)?;
        let question = Question::choice(&title, crate::native_sheet::file_name(path), &choices)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .ask(host, question, &mut input, &mut out)?;
        let path = copy(path)?;
        self.asked = Some((
            request,
            Asked::With {
                path,
                screens,
                keep,
            },
        ));
        Ok(())
    }

    /// Opens the book at `path` in the reading app of `screen` (empty: the
    /// one the host chooses), keeping the choice where `keep` says so.
    pub(super) fn open_in(
        &self,
        host: &Host<'_>,
        path: &str,
        screen: &str,
        keep: Option<&str>,
    ) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let path = Param::new(PATH, path)?;
        let handover = match keep {
            Some(keep) => {
                let params = [path, Param::new(KEEP, keep)?];
                self.ui()?.handover(host, &params, &mut input, &mut out)?
            }
            None => self.ui()?.handover(host, &[path], &mut input, &mut out)?,
        };
        let open = if screen.is_empty() {
            Open::slot("core.reader")?
        } else {
            Open::in_slot(screen, "core.reader")?
        };
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(open.handover(handover))
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }
}
