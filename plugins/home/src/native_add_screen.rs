//! The *Add* screen, in two steps like interface 6: the kinds (and *Add a
//! page*, *Remove this page*, *Reset landscape layout*), then the things of one
//! kind as a list. What each choice writes is `native_add`; this file reads the
//! device, draws the rows and stages the writes through the host: the
//! settings, the placements reset, the message and the way back. Any error
//! drops everything staged (`docs/abi/screens-input.md` 2.1), so a choice is
//! all or nothing.
//!
//! The things of a widget kind are a gallery (`native_gallery`): the widgets
//! drawn as they would show, composed in by the host. Where the host composes
//! nothing (the X4 until its stack gate) they are the list, as the tools are.
//!
//! Not here (ignored acceptance cases, `PORTING-7.md`): the ready-made layouts,
//! icons (#517).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode, ResetTarget},
        fonts::{Font, FontRole},
        settings::SettingValue,
        snapshot,
        ui::{HandoverId, Notify, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    ListFocus, ListInput, ListRenderer, Messages, Page, Paint, Style, focus, geometry, header,
    mem::{SERVICE_BUFFER, buffer, push},
    paint,
    text::{copy, decimal},
};

use crate::native_add::{self as add, Entry, Layout, Plan, Row, TOOLS_KIND};
use crate::native_gallery::{self as gallery, Drawn, Gallery, Stage};
use crate::native_grid::{Sizes, What};
use crate::native_made::Made;
use crate::native_measure as measure;
use crate::native_pages::{self, Write};
use crate::native_shown::Shown;

/// The screen this is declared as.
pub const SCREEN: &str = "home.add-widget";
/// Parameter: the page added to (0 the first).
const PAGE: &str = "page";
/// Parameter: the kind shown.
const KIND: &str = "kind";
/// Room for a command besides its text.
const OVERHEAD: usize = 512;

mod view;

/// How the things of a kind show.
#[derive(Debug)]
enum Mode {
    /// As rows: the kinds, the tools, and the widgets where the host composes
    /// nothing.
    List,
    /// The widgets of a kind: the gallery is laid out at the first draw or input
    /// (it measures them, which needs the screen's own callbacks).
    Pending,
    /// The widgets drawn as they would show.
    Gallery(Gallery<Made>),
}

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
    mode: Mode,
    /// The arrow keys stay on the gallery's page.
    arrows_stay: bool,
    /// The header's tap regions in the gallery.
    header: Page,
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
            Some(on) => native_pages::expand(&shown.src, &on.items)?,
            None => Vec::new(),
        };
        let entries = add::entries(
            shown.src.widget_rows(),
            shown.src.tool_rows(),
            native_pages::Source::status_bar(&shown.src),
            &on,
        )?;
        let rows = add::rows(
            kind.as_deref(),
            &entries,
            shown.pages.len(),
            native_pages::wide_kept(&shown.src)?,
        )?;
        let mut messages = Messages::new()?;
        let title = messages.copy(host, kind.as_deref().unwrap_or("home.edit.add"))?;
        let labels = Self::labels(host, &mut messages, &rows, &entries)?;
        let mode = match kind.as_deref() {
            Some(kind) if kind != TOOLS_KIND => Mode::Pending,
            _ => Mode::List,
        };
        Ok(Self {
            page,
            kind,
            title,
            entries,
            rows,
            labels,
            focus: ListFocus::default(),
            renderer: ListRenderer::default(),
            mode,
            arrows_stay: false,
            header: Page::default(),
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
                Row::AddPage => (messages.copy(host, "home.edit.add-page")?, String::new()),
                Row::RemovePage => (messages.copy(host, "home.edit.remove-page")?, String::new()),
                Row::ResetWide => (messages.copy(host, "home.edit.reset-wide")?, String::new()),
            };
            labels.push((name, value));
        }
        Ok(labels)
    }

    /// Draws the rows.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        self.ensure(host, ui)?;
        if matches!(self.mode, Mode::Gallery(_)) && self.draw_gallery(host, ui, frame)? {
            return Ok(());
        }
        // The renderer takes the key of the title and translates it itself.
        self.renderer.draw(
            host,
            frame,
            self.kind.as_deref().unwrap_or("home.edit.add"),
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
        self.ensure(host, ui)?;
        if matches!(self.mode, Mode::Gallery(_)) {
            return self.gallery_input(host, ui, input);
        }
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
        if let Some(What::Widget(id)) = &added {
            match measure::measure_widget(host, ui, id, measure::PLACE, &shown.frame.geometry) {
                Ok(Some((rows, least))) => shown.src.measured(id, rows, least)?,
                // Not measured (gone, refused): the grid's default of one row.
                Ok(None) => {}
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        let wide_grid = shown.wide_grid();
        let layout = Layout {
            wide: shown.wide,
            grid: &shown.frame.geometry,
            wide_grid: &wide_grid,
            pages: &shown.pages,
        };
        let src = &shown.src;
        let mut resets = Vec::new();
        let plan = match row {
            Row::Add(_) => match added {
                Some(what) => add::add_item(src, &layout, self.page, what)?,
                None => Plan::Nothing,
            },
            Row::AddPage => add::add_page(src, &layout)?,
            Row::RemovePage => {
                resets = add::removed_placements(src, &layout, self.page)?;
                add::remove_page(src, &layout, self.page)?
            }
            Row::ResetWide => add::reset_wide(src)?,
            Row::Kind(..) => Plan::Nothing,
        };
        self.stage(host, plan, &resets)
    }

    /// What an entry is added as: a widget as a placement of its own.
    fn placed(
        host: &Host<'_>,
        src: &impl native_pages::Source,
        entry: &Entry,
    ) -> Result<What, Error> {
        let What::Widget(id) = &entry.what else {
            return entry.what.try_clone();
        };
        Ok(What::Widget(crate::native_item_settings::placement(
            host, src, id,
        )?))
    }

    fn open_kind(&self, host: &Host<'_>, ui: &Ui, kind: &str) -> Result<(), Error> {
        open(host, ui, self.page, Some(kind))
    }

    /// Stages a plan and the way back to the page (past the kinds too). No room,
    /// and a change too big for one callback, say so and keep the screen.
    fn stage(&self, host: &Host<'_>, plan: Plan, resets: &[String]) -> Result<(), Error> {
        let writes = match plan {
            Plan::Nothing => return Ok(()),
            Plan::NoRoom => return say(host, "home.grid.no-room"),
            Plan::Writes(writes) => writes,
        };
        if !apply(host, &writes, resets)? {
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

/// Stages the settings written and the placements reset, then the messages.
/// Over the host's budget for one callback nothing is staged: the old layout
/// stays, the reader is told, and `false` comes back.
///
/// # Errors
/// Allocation and host errors; an error return drops everything staged.
pub fn apply(host: &Host<'_>, writes: &[Write], resets: &[String]) -> Result<bool, Error> {
    if !add::fits(writes, resets) {
        say(host, "home.grid.too-big")?;
        return Ok(false);
    }
    let longest = writes
        .iter()
        .map(|write| match write {
            Write::Set { text, .. } => text.len(),
            Write::Notify(_) => 0,
        })
        .max()
        .unwrap_or(0);
    let mut scratch = buffer(OVERHEAD + longest)?;
    let mut sender = CommandSender::new(host, &mut scratch);
    for write in writes {
        if let Write::Set { setting, text } = write {
            sender.set_setting(setting, SettingValue::Text(text))?;
        }
    }
    for id in resets {
        sender.reset_setting(ResetTarget::Placement(id))?;
    }
    for write in writes {
        if let Write::Notify(key) = write {
            say(host, key)?;
        }
    }
    Ok(true)
}

/// Says the message of the catalog key `key`.
///
/// # Errors
/// Allocation and host errors.
pub fn say(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text = Messages::new()?.copy(host, key)?;
    let mut scratch = buffer(OVERHEAD + text.len())?;
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests;
