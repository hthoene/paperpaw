//! What the reader's actions do besides turning pages: the font size a step
//! larger or smaller, the justification switched, and the start of the next or
//! the previous chapter. The settings are changed by command, so the host
//! applies them as the user's own change (and tells the reader, which lays the
//! page out again).
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    document::{DocCursor, DocId, Documents},
    settings::{self, SettingValue},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// Most entries of the table looked at (as the contents screen lists).
pub const MAX_ENTRIES: usize = 500;

/// The smallest and the largest reading size, in points (the setting's range;
/// a font pack may allow less, which the host applies).
pub const SIZES: (i64, i64) = (6, 40);

/// The size after `step` points, kept to what the setting allows.
#[must_use]
pub const fn stepped(size: i64, step: i64) -> i64 {
    let next = size.saturating_add(step);
    if next < SIZES.0 {
        SIZES.0
    } else if next > SIZES.1 {
        SIZES.1
    } else {
        next
    }
}

/// Asks the host to make the reading size `step` points larger or smaller.
///
/// # Errors
/// The host's refusals.
pub fn font_step(host: &Host<'_>, step: i64) -> Result<(), Error> {
    let mut bytes = [0; 32];
    let SettingValue::Int(size) = settings::read_setting(host, "core.font-size", &mut bytes)?
    else {
        return Err(Error::Invalid);
    };
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch)
        .set_setting("core.font-size", SettingValue::Int(stepped(size, step)))
}

/// Asks the host to switch the justification.
///
/// # Errors
/// The host's refusals.
pub fn toggle_justify(host: &Host<'_>) -> Result<(), Error> {
    let mut bytes = [0; 32];
    let on = matches!(
        settings::read_setting(host, "core.justify", &mut bytes)?,
        SettingValue::Bool(true)
    );
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch).set_setting("core.justify", SettingValue::Bool(!on))
}

/// The chapter start a step forward from `from` (the end of the page on show:
/// the first start at or after it) or back from it (the page's start: the last
/// start before it) leads to, among `starts`.
#[must_use]
pub fn choose(
    starts: impl Iterator<Item = (u32, u32)>,
    from: (u32, u32),
    forward: bool,
) -> Option<(u32, u32)> {
    if forward {
        starts.filter(|s| *s >= from).min()
    } else {
        starts.filter(|s| *s < from).max()
    }
}

/// Where the next (`forward`) or the previous chapter of document `id` begins,
/// seen from `from`; `None` where there is none. The starts of entries in other
/// sections are the host's places; those of the section of `from` are made
/// exact (`toc_target`), as Beta 3 did.
///
/// # Errors
/// `NoMemory`; a table the host cannot give is no chapter.
pub fn chapter(
    host: &Host<'_>,
    docs: &Documents,
    id: DocId,
    from: (u32, u32),
    forward: bool,
) -> Result<Option<(u32, u32)>, Error> {
    let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let (mut ask, mut reply) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let mut best: Option<(u32, u32)> = None;
    let mut cursor = DocCursor::start();
    let mut seen = 0;
    loop {
        // A table the host cannot give or hold is an error, not the chapter
        // found so far: going back could land on an earlier one.
        let page = match docs.toc(host, id, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NotFound) => return Ok(best),
            Err(error) => return Err(error),
        };
        for row in page.rows() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return Ok(best);
            }
            let at = row.position();
            let mut start = (at.section, at.offset);
            if row.approximate()
                && at.section == from.0
                && let Ok(exact) = docs.toc_target(host, id, row.index(), &mut ask, &mut reply)
            {
                start = (exact.section, exact.offset);
            }
            best = choose(
                best.into_iter().chain(core::iter::once(start)),
                from,
                forward,
            );
        }
        match page.end().next() {
            Some(next) => cursor = next,
            None => return Ok(best),
        }
    }
}

#[cfg(test)]
mod tests;
