//! Which of its lines a place shows: the lines to switch between are a
//! setting (ids separated by commas); a tap shows the next one, and the choice
//! is kept in `status-lines.txt` (top line, then bottom line, as interface 6
//! kept it, so the file is the same). Host-free; every allocation fallible.
use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text;

/// The file of the choice, in the data folder.
pub const FILE: &str = "status-lines.txt";
/// The most of it that is read; the choice is two short ids.
const MAX: usize = 1024;

/// The lines of a rotation (the setting's text), in order.
pub fn rotation(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|id| !id.is_empty())
}

/// The line showing: the one chosen last if it is still among the lines to
/// switch between, else the first; empty where there are none.
#[must_use]
pub fn current<'l>(list: &'l str, chosen: &str) -> &'l str {
    rotation(list)
        .find(|id| *id == chosen)
        .or_else(|| rotation(list).next())
        .unwrap_or("")
}

/// The line after the one showing, the first after the last; empty where there
/// are none.
#[must_use]
pub fn next<'l>(list: &'l str, chosen: &str) -> &'l str {
    let showing = current(list, chosen);
    let mut lines = rotation(list).skip_while(|id| *id != showing);
    lines.next();
    lines.next().or_else(|| rotation(list).next()).unwrap_or("")
}

/// The choice kept: the lines chosen last, top then bottom; a file that is
/// missing, too large or damaged is no choice (the first line of each place
/// shows).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn load(storage: &dyn Storage) -> Result<[String; 2], Error> {
    let mut bytes = [0u8; MAX + 1];
    let read = storage.read_at(FILE, 0, &mut bytes).unwrap_or(0);
    let Some(data) = bytes.get(..read).filter(|_| read <= MAX) else {
        return Ok([String::new(), String::new()]);
    };
    let text = core::str::from_utf8(data).unwrap_or("");
    let mut lines = text.lines().map(str::trim);
    let mut next = || text::copy(lines.next().unwrap_or(""));
    let top = next()?;
    Ok([top, next()?])
}

/// Keeps the choice. A write that fails is not told: the first line shows next
/// time.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn save(storage: &dyn Storage, chosen: &[String; 2]) -> Result<(), Error> {
    let mut out = String::new();
    out.try_reserve_exact(chosen[0].len() + chosen[1].len() + 2)
        .map_err(|_| Error::NoMemory)?;
    for line in chosen {
        out.push_str(line);
        out.push('\n');
    }
    storage.write(FILE, out.as_bytes()).ok();
    Ok(())
}

#[cfg(test)]
mod tests;
