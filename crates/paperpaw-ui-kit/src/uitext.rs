//! Whole host texts: a text card is read part by part within a stated limit.
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    ui::{TextId, TextWrite, Ui},
};

use crate::mem::{SERVICE_BUFFER, buffer};

/// The whole text `text` of `ui`, read part by part into one string.
///
/// # Errors
/// [`Error::NoSpace`] when the text is longer than `limit` bytes (nothing is
/// returned of it), [`Error::Invalid`] for a reply that does not progress, the
/// host's refusals, and [`Error::NoMemory`].
pub fn read_all(ui: &Ui, host: &Host<'_>, text: TextId, limit: usize) -> Result<String, Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut result = String::new();
    let mut offset = 0usize;
    loop {
        let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
        let part = ui.text_read(host, text, at, &mut input, &mut output)?;
        let total = usize::try_from(part.total()).map_err(|_| Error::NoSpace)?;
        if total > limit {
            return Err(Error::NoSpace);
        }
        let data = part.data();
        result
            .try_reserve(data.len())
            .map_err(|_| Error::NoMemory)?;
        result.push_str(data);
        offset = offset.checked_add(data.len()).ok_or(Error::NoSpace)?;
        if offset >= total {
            return Ok(result);
        }
        if data.is_empty() {
            return Err(Error::Invalid);
        }
    }
}

/// The most one part of a host text holds.
pub const PART_BYTES: usize = 3072;

/// Writes `text` to the host text `text_id` in parts of at most
/// [`PART_BYTES`] (cut at character boundaries), the last committing it. An
/// empty text is one write of no bytes.
///
/// # Errors
/// [`Error::NoSpace`] for an offset past `u32`, [`Error::NoMemory`], and the
/// host's refusals; a text begun is not committed then.
pub fn write_all(ui: &Ui, host: &Host<'_>, text_id: TextId, text: &str) -> Result<(), Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut offset = 0usize;
    loop {
        let mut end = text.len().min(offset + PART_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let last = end == text.len();
        ui.text_write(
            host,
            text_id,
            u32::try_from(offset).map_err(|_| Error::NoSpace)?,
            text.get(offset..end).unwrap_or_default(),
            if last {
                TextWrite::Last
            } else {
                TextWrite::Continue
            },
            &mut input,
            &mut output,
        )?;
        offset = end;
        if last {
            return Ok(());
        }
    }
}
