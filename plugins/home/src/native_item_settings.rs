//! The options' *Settings* entry of a widget on the home screen: whether the
//! widget has settings of its own, the placement it gets (each placement has
//! settings of its own; one placed before there were placements gets its number
//! first) and the settings opened on that placement. As interface 6 did it.
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    registry::{PageCursor, Registry, ValuesQuery},
    ui::{Open, Param, Ui},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

use crate::native_add as add;
use crate::native_pages::{self, Source};

/// The slot the settings fill.
const SETTINGS_SLOT: &str = "core.settings";
/// The hand-over parameter that names the group or placement to show.
const ID: &str = "id";

/// Whether widget `id` has settings of its own: a settings group with the
/// widget's id (its placement's number left out) that holds settings.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn has_settings(host: &Host<'_>, id: &str) -> Result<bool, Error> {
    let registry = Registry::open(host)?;
    let found = (|| {
        let mut input = buffer(512)?;
        let mut out = buffer(SERVICE_BUFFER)?;
        let page = registry.settings(
            host,
            native_pages::base(id),
            PageCursor::start(),
            &mut input,
            &mut out,
        )?;
        Ok(page.rows().next().is_some())
    })();
    let released = registry.release(host);
    let found = found?;
    released?;
    Ok(found)
}

/// The placement whose settings `id` shows: itself where it is one already
/// (`id@n`), else the first number no page holds and no values are kept for.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn placement(host: &Host<'_>, src: &impl Source, id: &str) -> Result<String, Error> {
    if id.contains('@') {
        return paperpaw_ui_kit::text::copy(id);
    }
    let registry = Registry::open(host)?;
    let found = (|| {
        let mut input = buffer(512)?;
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
    Ok(placed)
}

/// Opens the settings on `placement`'s page.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn open(host: &Host<'_>, ui: &Ui, placement: &str) -> Result<(), Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut out = buffer(SERVICE_BUFFER)?;
    let handover = ui.handover(host, &[Param::new(ID, placement)?], &mut input, &mut out)?;
    let mut scratch = buffer(SERVICE_BUFFER)?;
    CommandSender::new(host, &mut scratch).open(Open::slot(SETTINGS_SLOT)?.handover(handover))
}
