//! CalDAV and iCalendar for plugins that keep tasks or a calendar on a
//! server: what is the same for every such plugin, with no host and no I/O.
//!
//! - [`ical`]: content lines unfolded as they stream ([`ical::Lines`]), a
//!   property's name, parameters and value, text escaping, and a to-do read
//!   ([`ical::TodoReader`]) and written ([`ical::write_todo`]).
//! - [`patch`]: a to-do changed in place as its file streams through
//!   ([`patch::Patcher`]): only the properties a change names are replaced,
//!   everything else (alarms, descriptions, what other programs wrote) is
//!   passed on byte for byte.
//! - [`xml`]: a small streaming scanner for the answers of a server.
//! - [`dav`]: a multistatus answer read as it streams ([`dav::Multistatus`]),
//!   the bodies of the requests, addresses and ETags.
//! - [`date`]: dates and times as iCalendar writes them.
//!
//! Everything works on pieces of any size, keeps bounded memory and refuses
//! nothing a server may send: what it does not know it skips.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod date;
pub mod dav;
pub mod ical;
pub mod patch;
pub mod xml;

/// Why something could not be read or made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The heap could not give the room.
    NoMemory,
    /// Not what it should be (a date, an address).
    Invalid,
}

/// Appends `text` to `out` in memory that may be refused.
pub(crate) fn push_str(out: &mut alloc::string::String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}
