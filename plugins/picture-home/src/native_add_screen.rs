//! The *Add* screen, in two steps like interface 6: the kinds (and *Add a
//! page*, *Remove this page*), then the things of one kind as a list. What each
//! choice writes is `native_add`; this file reads the device, draws the rows and
//! stages the writes through the host (`native_stage`): the settings, the
//! placements reset, the message and the way back. Any error drops everything
//! staged (`docs/abi/screens-input.md` 2.1), so a choice is all or nothing.
//!
//! Not here (`PORTING-7.md`): the icons of the rows (the kit's list renderer
//! draws none).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        registry::{PageCursor, Registry, ValuesQuery},
        ui::{HandoverId, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    ListFocus, ListInput, ListRenderer, Messages, focus,
    mem::{SERVICE_BUFFER, buffer},
    text::{copy, decimal},
};

use crate::native_add::{self as add, Entry, Layout, Plan, Row};
use crate::native_grid::What;
use crate::native_measure as measure;
use crate::native_pages::Source;
use crate::native_shown::Shown;
use crate::native_stage as stage;

/// The screen this is declared as.
pub const SCREEN: &str = "picture-home.add";
/// Parameter: the page added to (0 the first).
const PAGE: &str = "page";
/// Parameter: the kind shown.
const KIND: &str = "kind";
/// Room for a command besides its text.
const OVERHEAD: usize = 512;

/// The *Add* screen with what it lists.
#[derive(Debug)]
pub struct AddScreen {
    page: usize,
    kind: Option<String>,
    title: String,
    entries: Vec<Entry>,
    rows: Vec<Row>,
    /// What each row shows: its name and its value.
    labels: Vec<(String, String)>,
    focus: ListFocus,
    renderer: ListRenderer,
}

/// The page and kind a handover names. A page that is not a number is the
/// first (as on interface 6); a repeated key must not differ.
///
/// # Errors
/// [`Error::Invalid`] for a key given twice differently; [`Error::NoMemory`].
pub fn params<'a>(
    params: impl Iterator<Item = Result<(&'a str, &'a str), Error>>,
) -> Result<(usize, Option<String>), Error> {
    let (mut page, mut kind): (Option<&str>, Option<&str>) = (None, None);
    for param in params {
        let (key, value) = param?;
        let seen = match key {
            PAGE => &mut page,
            KIND => &mut kind,
            _ => continue,
        };
        if seen.is_some_and(|seen| seen != value) {
            return Err(Error::Invalid);
        }
        *seen = Some(value);
    }
    let page = page.and_then(|page| page.parse().ok()).unwrap_or(0);
    let kind = kind.filter(|kind| !kind.is_empty()).map(copy).transpose()?;
    Ok((page, kind))
}

/// Opens the screen to add to page number `page` (0 the first), at the kinds or
/// at the things of `kind`.
///
/// # Errors
/// Allocation and host errors.
pub fn open(host: &Host<'_>, ui: &Ui, page: usize, kind: Option<&str>) -> Result<(), Error> {
    let mut scratch = buffer(SERVICE_BUFFER)?;
    let mut out = buffer(SERVICE_BUFFER)?;
    let mut digits = [0u8; 20];
    let page = Param::new(PAGE, decimal(page, &mut digits))?;
    let handover: HandoverId = match kind {
        Some(kind) => ui.handover(
            host,
            &[page, Param::new(KIND, kind)?],
            &mut scratch,
            &mut out,
        )?,
        None => ui.handover(host, &[page], &mut scratch, &mut out)?,
    };
    CommandSender::new(host, &mut scratch).open(Open::screen(SCREEN)?.handover(handover))
}

impl AddScreen {
    /// Reads what there is to add to page number `page`, and the rows of the
    /// step `kind` says.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn new(host: &Host<'_>, page: usize, kind: Option<String>) -> Result<Self, Error> {
        let shown = Shown::current(host)?;
        let on = match shown.pages.get(page) {
            Some(on) => crate::native_pages::expand(&shown.src, &on.items)?,
            None => Vec::new(),
        };
        let entries = add::entries(shown.src.widget_rows(), shown.src.tool_rows(), &on)?;
        let rows = add::rows(kind.as_deref(), &entries, shown.pages.len())?;
        let mut messages = Messages::new()?;
        let title = messages.copy(host, kind.as_deref().unwrap_or("picture-home.add"))?;
        let labels = Self::labels(host, &mut messages, &rows, &entries)?;
        Ok(Self {
            page,
            kind,
            title,
            entries,
            rows,
            labels,
            focus: ListFocus::default(),
            renderer: ListRenderer::default(),
        })
    }

    fn labels(
        host: &Host<'_>,
        messages: &mut Messages,
        rows: &[Row],
        entries: &[Entry],
    ) -> Result<Vec<(String, String)>, Error> {
        let mut labels = Vec::new();
        labels
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for row in rows {
            let (name, value) = match row {
                Row::Kind(kind, count) => {
                    let mut digits = [0u8; 20];
                    (
                        messages.copy(host, kind)?,
                        copy(decimal(*count, &mut digits))?,
                    )
                }
                Row::Add(index) => {
                    let title = entries.get(*index).map_or("", |e| e.title.as_str());
                    (copy(title)?, copy("+")?)
                }
                Row::AddPage => (messages.copy(host, "picture-home.add-page")?, String::new()),
                Row::RemovePage => (
                    messages.copy(host, "picture-home.remove-page")?,
                    String::new(),
                ),
            };
            labels.push((name, value));
        }
        Ok(labels)
    }

    /// Draws the rows.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        self.renderer.draw(
            host,
            frame,
            &self.title,
            &self.labels,
            self.focus.focus(),
            true,
        )
    }

    /// Takes an input: the list moves, a chosen row opens the kind or changes
    /// the pages.
    ///
    /// # Errors
    /// Allocation and host errors; nothing is staged then.
    pub fn input(&mut self, host: &Host<'_>, ui: &Ui, input: &Input<'_>) -> Result<Handled, Error> {
        match self
            .focus
            .input(host, &self.renderer, self.rows.len(), input)?
        {
            ListInput::Chosen(index) => {
                self.choose(host, ui, index)?;
                Ok(Handled::Yes)
            }
            other => Ok(focus::handled(other)),
        }
    }

    fn choose(&self, host: &Host<'_>, ui: &Ui, index: usize) -> Result<(), Error> {
        let Some(row) = self.rows.get(index) else {
            return Ok(());
        };
        if let Row::Kind(kind, _) = row {
            return self.open_kind(host, ui, kind);
        }
        let mut shown = Shown::current(host)?;
        // What is added is measured at the grid's width, so it gets the rows it
        // needs.
        let added = match row {
            Row::Add(entry) => match self.entries.get(*entry) {
                Some(entry) => Some(Self::placed(host, &shown.src, entry)?),
                None => None,
            },
            _ => None,
        };
        if let (false, Some(What::Widget(id))) = (shown.wide, &added) {
            match measure::measure_widget(host, ui, id, measure::PLACE, &shown.frame.geometry) {
                Ok(Some((rows, least))) => shown.src.measured(id, rows, least)?,
                // Not measured (gone, refused): the grid's default of one row.
                Ok(None) => {}
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        let layout = Layout::new(shown.wide, &shown.frame.geometry, &shown.pages);
        let src = &shown.src;
        let mut resets = Vec::new();
        let plan = match row {
            Row::Add(_) => match added {
                Some(what) => add::add_item(src, &layout, self.page, what)?,
                None => Plan::Nothing,
            },
            Row::AddPage => add::add_page(src, &layout)?,
            Row::RemovePage => {
                resets = add::removed_placements(&layout, self.page)?;
                add::remove_page(src, &layout, self.page)?
            }
            Row::Kind(..) => Plan::Nothing,
        };
        self.stage(host, plan, &resets)
    }

    /// What an entry is added as: a widget as a placement of its own.
    fn placed(host: &Host<'_>, src: &impl Source, entry: &Entry) -> Result<What, Error> {
        let What::Widget(id) = &entry.what else {
            return entry.what.try_clone();
        };
        let registry = Registry::open(host)?;
        let found = (|| {
            let mut input = buffer(OVERHEAD)?;
            let mut out = buffer(SERVICE_BUFFER)?;
            add::placement(src, id, |placed| {
                let page = registry.values(
                    host,
                    ValuesQuery::Placement(placed),
                    PageCursor::start(),
                    &mut input,
                    &mut out,
                )?;
                Ok(page.rows().next().is_some())
            })
        })();
        let released = registry.release(host);
        let placed = found?;
        released?;
        Ok(What::Widget(placed))
    }

    fn open_kind(&self, host: &Host<'_>, ui: &Ui, kind: &str) -> Result<(), Error> {
        open(host, ui, self.page, Some(kind))
    }

    /// Stages a plan and the way back to the page (past the kinds too). No room,
    /// and a change too big for one callback, say so and keep the screen.
    fn stage(&self, host: &Host<'_>, plan: Plan, resets: &[String]) -> Result<(), Error> {
        let writes = match plan {
            Plan::Nothing => return Ok(()),
            Plan::NoRoom => return stage::say(host, "picture-home.no-room"),
            Plan::Upright => return stage::say(host, "picture-home.edit-upright"),
            Plan::Writes(writes) => writes,
        };
        if !stage::apply(host, &writes, resets)? {
            return Ok(());
        }
        let mut scratch = [0; OVERHEAD];
        let mut sender = CommandSender::new(host, &mut scratch);
        sender.back()?;
        if self.kind.is_some() {
            sender.back()?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests;
