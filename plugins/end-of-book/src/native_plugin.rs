//! The flat End of book plugin: the book events and one list screen.
use crate::{
    RATING, declarations,
    native_model::{self, Next, Row, Texts},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        formatting::Arg,
        library::{BookRef, Library, Query},
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        ui::{HandoverId, Instance, Notify, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/end_of_book_declarations.rs"));
}

const EVENTS: u32 = declarations::EVENTS_TOKEN;
const SCREEN: u32 = declarations::SCREEN_TOKEN;
/// Books of a series stepped over at most to find one not read yet.
const MOST_STEPS: usize = 64;

/// The book whose end was reached, and whether it counts as finished (what
/// the screen's third row says), until the book closes.
#[derive(Debug)]
struct Session {
    path: String,
    finished: bool,
}

/// What the rating row knows. Reading a book's fields back (LIB op 22) has
/// no SDK facade yet, so a rating already given is not known here and the
/// row refuses a press rather than overwrite it (see `PORTING-7.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "`Stars` is read from the book's field once the SDK can (PORTING-7.md)"
)]
enum Rating {
    Unknown,
    Stars(u8),
}

/// One open screen.
#[derive(Debug)]
struct View {
    path: String,
    title: String,
    next: Option<Next>,
    rating: Rating,
    finished: bool,
    rows: Vec<Row>,
    list: ItemList,
}

/// The interface-7 End of book plugin.
#[derive(Debug, Default)]
pub struct NativeEndOfBook {
    library: Option<Library>,
    ui: Option<Ui>,
    kept: Option<Session>,
    screens: Screens<View>,
}

impl Plugin for NativeEndOfBook {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        // Only the kinds this plugin handles are read, so only their decoders are linked.
        if token == EVENTS {
            if let Some(Event::Book { reading, .. }) =
                incoming.decode_as::<kinds::BookFinished>()?
            {
                return self.finished(host, reading.path());
            }
            if let Some(Event::Book { reading, .. }) = incoming.decode_as::<kinds::BookClosed>()? {
                return self.closed(host, reading.path());
            }
        }
        if token == SCREEN {
            if let Some(Event::ScreenOpened {
                instance, handover, ..
            }) = incoming.decode_as::<kinds::ScreenOpened>()?
            {
                return self.opened(host, instance, handover);
            }
            if let Some(Event::ScreenShown {
                instance: Some(instance),
                ..
            }) = incoming.decode_as::<kinds::ScreenShown>()?
            {
                return self.screens.shown(SCREEN, instance);
            }
        }
        if let Some(Event::ScreenClosed { instance, .. }) =
            incoming.decode_as::<kinds::ScreenClosed>()?
        {
            self.screens.close(instance);
        }
        Ok(())
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = Messages::new()?.copy(host, "end-of-book.title")?;
        view.list
            .draw_with(host, &mut frame, &title, true, Footer::None)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step {
            self.choose(host, token, index)?;
        }
        Ok(ItemList::handled(step))
    }
}

/// Whether a switch is on; the declared default is on.
fn switch(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut bytes = [0; 32];
    Ok(!matches!(
        settings::read_setting(host, key, &mut bytes)?,
        SettingValue::Bool(false)
    ))
}

impl NativeEndOfBook {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn library(&self) -> Result<&Library, Error> {
        self.library.as_ref().ok_or(Error::State)
    }

    /// The end of a book: it counts as finished as the setting says, and
    /// the screen opens unless that is off. A book the library does not
    /// have (gone from the card) has no end to speak of.
    fn finished(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match self
            .library()?
            .book(host, BookRef::path(path)?, &mut input, &mut out)
        {
            Ok(_) => {}
            Err(Error::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        }
        let mark = switch(host, "end-of-book.mark")?;
        self.kept = Some(Session {
            path: copy(path)?,
            finished: mark,
        });
        self.library()?
            .set_finished(host, BookRef::path(path)?, mark, &mut input, &mut out)?;
        if switch(host, "end-of-book.show")? {
            let params = [Param::new("path", path)?];
            let handover = self
                .ui
                .as_ref()
                .ok_or(Error::State)?
                .handover(host, &params, &mut input, &mut out)?;
            let mut scratch = [0; 256];
            CommandSender::new(host, &mut scratch)
                .open(Open::screen("end-of-book.screen")?.handover(handover))?;
        }
        Ok(())
    }

    /// The reader's closing records how far the book is read: said again
    /// what the book is.
    fn closed(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let Some(session) = self.kept.take().filter(|s| s.path == path) else {
            return Ok(());
        };
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match self.library()?.set_finished(
            host,
            BookRef::path(path)?,
            session.finished,
            &mut input,
            &mut out,
        ) {
            Err(Error::NotFound) => Ok(()),
            other => other,
        }
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut path = String::new();
        if let Some(handover) = handover {
            for param in self
                .ui
                .as_ref()
                .ok_or(Error::State)?
                .params(host, handover, &mut input, &mut out)?
            {
                let param = param?;
                if param.key() == "path" {
                    path = copy(param.value())?;
                }
            }
        }
        let finished = match &self.kept {
            Some(session) if session.path == path => session.finished,
            _ => false,
        };
        let mut view = self.build(host, path, finished)?;
        let mut messages = Messages::new()?;
        view.refresh(host, &mut messages)?;
        self.screens.open(instance, view)
    }

    /// The facts of the book: its title, its place in a series and the next
    /// book there that is not read to the end.
    fn build(&self, host: &Host<'_>, path: String, finished: bool) -> Result<View, Error> {
        let library = self.library()?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let book = library.book(host, BookRef::path(&path)?, &mut input, &mut out)?;
        let title = copy(book.title())?;
        let series = copy(book.series())?;
        let (key, generation) = (book.key(), book.generation());
        let finished = finished || book.finished();
        let next = if series.trim().is_empty() {
            None
        } else {
            next_book(host, library, key, generation, &series)?
        };
        Ok(View {
            path,
            title,
            next,
            rating: Rating::Unknown,
            finished,
            rows: Vec::new(),
            list: ItemList::default(),
        })
    }

    fn choose(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        let Some(view) = self.screens.top(token) else {
            return Ok(());
        };
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut messages = Messages::new()?;
        match view.rows.get(index) {
            Some(Row::Next) => {
                let mut scratch = [0; 256];
                match &view.next {
                    Some(next) => {
                        // The reader the user bound to `core.reader` opens it:
                        // a handover with the path (`document.md` section 1).
                        let params = [Param::new("path", &next.path)?];
                        let handover = self
                            .ui
                            .as_ref()
                            .ok_or(Error::State)?
                            .handover(host, &params, &mut input, &mut out)?;
                        CommandSender::new(host, &mut scratch)
                            .open(Open::slot("core.reader")?.handover(handover))
                    }
                    None => CommandSender::new(host, &mut scratch).run_action("core.library"),
                }
            }
            Some(Row::Rate) => match view.rating {
                // BLOCKED (#228): the rating already given cannot be read back
                // until `Library::fields` (LIB op 22, #325) has an SDK facade,
                // so a press would overwrite it unseen. Said on screen, the
                // rating untouched; never faked.
                Rating::Unknown => {
                    let text = messages.copy(host, "end-of-book.rate.unknown")?;
                    let mut scratch = [0; 256];
                    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
                }
                Rating::Stars(stars) => {
                    let stars = native_model::next_stars(stars);
                    let value = native_model::field_value(stars)?;
                    library.set_field(
                        host,
                        BookRef::path(&view.path)?,
                        RATING,
                        &value,
                        &mut input,
                        &mut out,
                    )?;
                    view.rating = Rating::Stars(stars);
                    view.refresh(host, &mut messages)?;
                    redraw(host)
                }
            },
            Some(Row::Finished) => {
                // The view and the session follow only once the library took it.
                let finished = !view.finished;
                library.set_finished(
                    host,
                    BookRef::path(&view.path)?,
                    finished,
                    &mut input,
                    &mut out,
                )?;
                view.finished = finished;
                if let Some(session) = self.kept.as_mut().filter(|s| s.path == view.path) {
                    session.finished = finished;
                }
                view.refresh(host, &mut messages)?;
                redraw(host)
            }
            None => Ok(()),
        }
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
}

impl View {
    /// The rows as the state says them, in the reader's language.
    fn refresh(&mut self, host: &Host<'_>, messages: &mut Messages) -> Result<(), Error> {
        let none = messages.copy(host, "end-of-book.rate.none")?;
        let rate_value = match self.rating {
            Rating::Unknown => String::new(),
            Rating::Stars(stars) => native_model::stars_text(stars, &none)?,
        };
        let next_note = match &self.next {
            Some(next) if next.number.is_empty() => messages.format(
                host,
                "end-of-book.next.note.plain",
                &[Arg::Text {
                    name: "title",
                    value: &next.title,
                }],
            )?,
            Some(next) => messages.format(
                host,
                "end-of-book.next.note",
                &[
                    Arg::Text {
                        name: "title",
                        value: &next.title,
                    },
                    Arg::Text {
                        name: "number",
                        value: &next.number,
                    },
                    Arg::Int {
                        name: "total",
                        value: i64::from(next.total),
                    },
                ],
            )?,
            None => messages.copy(host, "end-of-book.library.note")?,
        };
        let texts = Texts {
            next_title: messages.copy(
                host,
                if self.next.is_some() {
                    "end-of-book.next"
                } else {
                    "end-of-book.library"
                },
            )?,
            next_note,
            rate_title: messages.copy(host, "end-of-book.rate")?,
            rate_value,
            rate_note: messages.format(
                host,
                "end-of-book.rate.note",
                &[Arg::Text {
                    name: "title",
                    value: &self.title,
                }],
            )?,
            finished_title: messages.copy(host, "end-of-book.finished")?,
            finished_value: messages.copy(
                host,
                if self.finished {
                    "end-of-book.finished.yes"
                } else {
                    "end-of-book.finished.no"
                },
            )?,
            finished_note: messages.copy(host, "end-of-book.finished.note")?,
        };
        let mut kinds = Vec::new();
        let mut items = Vec::new();
        for (kind, item) in native_model::rows(&texts)? {
            push(&mut kinds, kind)?;
            push(&mut items, item)?;
        }
        self.rows = kinds;
        self.list.replace(items);
        Ok(())
    }
}

/// The first book after the one with `key` in its series that is not read to
/// the end, and how many books the series has. The host says which book
/// follows (LIB op 6); a finished one is stepped over, at most
/// [`MOST_STEPS`] times.
fn next_book(
    host: &Host<'_>,
    library: &Library,
    key: u32,
    generation: u32,
    series: &str,
) -> Result<Option<Next>, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut from = (key, generation);
    for _ in 0..MOST_STEPS {
        let entry = match library.next_in_series(
            host,
            BookRef::catalog(from.0, from.1)?,
            &mut input,
            &mut out,
        ) {
            Ok(entry) => entry,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let (next_key, finished) = (entry.key(), entry.finished());
        from = (next_key, generation);
        if finished {
            continue;
        }
        let (title, number) = (copy(entry.title())?, copy(entry.number())?);
        let book = library.book(
            host,
            BookRef::catalog(next_key, generation)?,
            &mut input,
            &mut out,
        )?;
        let path = copy(book.path())?;
        let query = Query::new().series(series)?;
        let total = library
            .query(host, &query, &mut input, &mut out)?
            .end()
            .total();
        return Ok(Some(Next {
            path,
            title,
            number,
            total,
        }));
    }
    Ok(None)
}
