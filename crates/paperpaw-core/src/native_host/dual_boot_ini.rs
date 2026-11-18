//! The `dual-boot` setting of `paperpaw.ini`'s `[firmware]` section as the
//! host reads and writes it for `command.firmware_dual_boot`
//! (`docs/abi/device.md` 5.3 "Dual boot", `docs/dual-boot.md`).
//!
//! * **Read whole, written whole.** The file is read whole (a file that
//!   cannot be read, is over [`INI_MAX`] bytes or is not UTF-8 is a
//!   [`Failure`] and nothing is written), the `dual-boot` of the last
//!   `[firmware]` section is set (the section is added where there is none,
//!   the rest of the file stays as it was, line endings included), and
//!   [`Storage::write`] replaces the file, which is atomic where the medium
//!   allows: a power cut leaves the old or the new file, never half of one.
//! * **The same reading as the firmware's.** One reader decides everything
//!   here (the choice, where the key is set, whether an edit worked), and it
//!   reads as the X4's own (`paperpaw_x4::dual_boot::IniScan`) does: line by
//!   line, a header as `[name]`, an entry by `paperpaw_config::parse` (quotes,
//!   escapes, comments), a line over [`MAX_LINE`] bytes by its start only (a
//!   header leaves the section, a `dual-boot` entry guards, any other line
//!   changes nothing), a line that is no UTF-8 skipped. The last `dual-boot`
//!   of a `[firmware]` section wins; `off`, `false`, `no` and `0` mean off
//!   (any case), `auto` or nothing mean auto, anything else means on (a typo
//!   of the guard must not leave the other firmware open to an update).
//! * **Checked after the edit.** The edited bytes are read again by the same
//!   reader and are written only if they say the mode asked for: else
//!   [`Failure::Unexpected`] and nothing is written.
//! * Allocation is fallible and bounded by the file's size and a few lines.
use alloc::vec::Vec;
use paperpaw_platform::Storage;

/// The file, in the card's root.
pub(crate) const INI_PATH: &str = "paperpaw.ini";

/// The longest file read whole, in bytes: on the X4 it is read and the
/// edited copy built in the same heap.
pub(crate) const INI_MAX: usize = 16 * 1024;

/// The section and the key.
const SECTION: &str = "firmware";
const KEY: &str = "dual-boot";

/// What a read, an edit or the write failed on (`install_done` reason 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The file cannot be read whole, is over [`INI_MAX`] or is not UTF-8.
    Unreadable,
    /// No room for the edited copy.
    NoMemory,
    /// The card refused the write; the old file is as it was.
    Write,
    /// The edited file does not read as the mode asked for (a line the editor
    /// and the firmware read differently); nothing is written.
    Unexpected,
}

/// The value written for `command.firmware_dual_boot`'s `mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// `mode` 0: `dual-boot = auto`.
    Auto,
    /// `mode` 1: `dual-boot = off`.
    Off,
}

impl Mode {
    /// The mode `mode` names; `None` for any other number.
    pub(crate) fn of(mode: u32) -> Option<Self> {
        match mode {
            0 => Some(Self::Auto),
            1 => Some(Self::Off),
            _ => None,
        }
    }

    /// What the reader reads of the line this mode writes.
    fn choice(self) -> Choice {
        match self {
            Self::Auto => Choice::Auto,
            Self::Off => Choice::Off,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
        }
    }
}

/// What the file chooses, as the firmware reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Choice {
    /// Not set, or `auto`.
    #[default]
    Auto,
    On,
    Off,
}

impl Choice {
    fn of(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("auto") {
            return Self::Auto;
        }
        if ["off", "false", "no", "0"]
            .iter()
            .any(|off| value.eq_ignore_ascii_case(off))
        {
            return Self::Off;
        }
        Self::On
    }
}

/// The longest line the firmware's reader takes whole; a longer one counts by
/// its start only (`paperpaw_x4::dual_boot`).
pub(crate) const MAX_LINE: usize = 256;

/// What one line of the file is to the reader.
enum Seen {
    /// `[name]`: whether the section is `[firmware]`.
    Header(bool),
    /// A `dual-boot` entry, and what it chooses (a line too long to be read
    /// chooses on: the safe way).
    Key(Choice),
    Other,
}

/// `line` (without its ending) as the firmware's reader sees it: every `\r`
/// is dropped (`IniScan` takes none into a line), and only what is left counts
/// toward [`MAX_LINE`].
fn read_line(line: &[u8]) -> Seen {
    let kept = line.iter().filter(|byte| **byte != b'\r').count();
    if kept > MAX_LINE {
        // Too long to be one of ours: by its start only.
        let mut bytes = line
            .iter()
            .copied()
            .filter(|byte| *byte != b'\r')
            .take(MAX_LINE);
        let first = bytes.find(|byte| !byte.is_ascii_whitespace());
        return match first {
            Some(b'[') => Seen::Header(false),
            Some(first) => {
                // Only the first MAX_LINE bytes kept are read (the reader's
                // buffer is cut there).
                let mut head = [0_u8; KEY.len()];
                head[0] = first;
                let mut got = 1;
                for byte in bytes.take(KEY.len() - 1) {
                    head[got] = byte;
                    got += 1;
                }
                if got == KEY.len() && head.as_slice() == KEY.as_bytes() {
                    Seen::Key(Choice::On)
                } else {
                    Seen::Other
                }
            }
            None => Seen::Other,
        };
    }
    // A stray `\r` inside the line is dropped as well (a trailing one
    // included): the line is copied only then.
    let mut clean = [0_u8; MAX_LINE];
    let line = if kept == line.len() {
        line
    } else {
        let mut at = 0;
        for byte in line.iter().filter(|byte| **byte != b'\r') {
            clean[at] = *byte;
            at += 1;
        }
        &clean[..at]
    };
    let Ok(line) = core::str::from_utf8(line) else {
        return Seen::Other;
    };
    let line = line.trim();
    if let Some(header) = line.strip_prefix('[') {
        return Seen::Header(
            header
                .split_once(']')
                .is_some_and(|(name, _)| name.trim() == SECTION),
        );
    }
    if line.is_empty() {
        return Seen::Other;
    }
    let mut seen = Seen::Other;
    for entry in paperpaw_config::parse(line).flatten() {
        if entry.key == KEY {
            seen = Seen::Key(Choice::of(&entry.value.as_str()));
        }
    }
    seen
}

/// What `bytes` choose: the last `dual-boot` of a `[firmware]` section.
pub(crate) fn choice_of(bytes: &[u8]) -> Choice {
    // A byte order mark some editors put first.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let mut inside = false;
    let mut choice = Choice::Auto;
    for line in bytes.split(|byte| *byte == b'\n') {
        match read_line(line) {
            Seen::Header(firmware) => inside = firmware,
            Seen::Key(value) if inside => choice = value,
            _ => {}
        }
    }
    choice
}

/// What `text` chooses (see [`choice_of`]).
#[cfg(test)]
pub(crate) fn choice(text: &str) -> Choice {
    choice_of(text.as_bytes())
}

/// Where `dual-boot` is set in `text` and where the section ends.
struct Places {
    /// The last `dual-boot` line inside a `[firmware]` section.
    key: Option<usize>,
    /// The line after the last non-blank line of the last `[firmware]`
    /// section; `None` where there is no such section.
    end: Option<usize>,
}

fn places(text: &str) -> Places {
    let mut inside = false;
    let mut places = Places {
        key: None,
        end: None,
    };
    for (index, line) in text.split_inclusive('\n').enumerate() {
        // The first line may carry a byte order mark, as the reader skips it.
        let bytes = line.as_bytes();
        let bytes = if index == 0 {
            bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
        } else {
            bytes
        };
        let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
        match read_line(bytes) {
            Seen::Header(firmware) => {
                inside = firmware;
                if inside {
                    // A newer section starts: its own end and key.
                    places.end = Some(index + 1);
                    places.key = None;
                }
            }
            Seen::Key(_) if inside => {
                places.key = Some(index);
                places.end = Some(index + 1);
            }
            _ if inside && !line.trim().is_empty() => places.end = Some(index + 1),
            _ => {}
        }
    }
    places
}

/// Appends `dual-boot = <mode>` and `eol` (no room is `out`'s own growth: it
/// was reserved for the line).
fn key_line(out: &mut Vec<u8>, mode: Mode, eol: &str) {
    out.extend_from_slice(KEY.as_bytes());
    out.extend_from_slice(b" = ");
    out.extend_from_slice(mode.word().as_bytes());
    out.extend_from_slice(eol.as_bytes());
}

/// `text` with `dual-boot` set to `mode` in the last `[firmware]` section
/// (added at the end where there is none), the rest unchanged.
///
/// # Errors
/// [`Failure::Unreadable`] over [`INI_MAX`] bytes, [`Failure::NoMemory`].
pub(crate) fn edit(text: &str, mode: Mode) -> Result<Vec<u8>, Failure> {
    if text.len() > INI_MAX {
        return Err(Failure::Unreadable);
    }
    let mut out = Vec::new();
    // The text, a new section header, one key line and the endings.
    out.try_reserve_exact(text.len() + SECTION.len() + KEY.len() + 24)
        .map_err(|_| Failure::NoMemory)?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let places = places(text);
    let count = text.split_inclusive('\n').count();
    for (index, old) in text.split_inclusive('\n').enumerate() {
        if places.key == Some(index) {
            // The line's own ending stays.
            let ending = &old[old.trim_end_matches(['\r', '\n']).len()..];
            key_line(&mut out, mode, ending);
            continue;
        }
        if places.key.is_none() && places.end == Some(index) {
            key_line(&mut out, mode, eol);
        }
        out.extend_from_slice(old.as_bytes());
        if places.key.is_none() && places.end == Some(index + 1) && !old.ends_with('\n') {
            // The line before the new one has no ending: it gets one.
            out.extend_from_slice(eol.as_bytes());
        }
    }
    match (places.key, places.end) {
        (None, Some(end)) if end == count => key_line(&mut out, mode, eol),
        (None, None) => {
            if !text.is_empty() && !text.ends_with('\n') {
                out.extend_from_slice(eol.as_bytes());
            }
            out.extend_from_slice(b"[firmware]");
            out.extend_from_slice(eol.as_bytes());
            key_line(&mut out, mode, eol);
        }
        _ => {}
    }
    // Written only if the firmware's own reader reads what was asked for.
    if choice_of(&out) != mode.choice() {
        return Err(Failure::Unexpected);
    }
    Ok(out)
}

/// Reads `paperpaw.ini` whole; a missing file is an empty one.
///
/// # Errors
/// [`Failure::Unreadable`], [`Failure::NoMemory`].
pub(crate) fn read(storage: &dyn Storage) -> Result<Vec<u8>, Failure> {
    let size = match storage.size(INI_PATH) {
        Ok(size) => size,
        Err(paperpaw_platform::StorageError::NotFound) => return Ok(Vec::new()),
        Err(_) => return Err(Failure::Unreadable),
    };
    let size = usize::try_from(size)
        .ok()
        .filter(|size| *size <= INI_MAX)
        .ok_or(Failure::Unreadable)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| Failure::NoMemory)?;
    bytes.resize(size, 0);
    let mut done = 0;
    while done < size {
        let read = storage
            .read_at(INI_PATH, done as u64, &mut bytes[done..])
            .map_err(|_| Failure::Unreadable)?;
        if read == 0 {
            return Err(Failure::Unreadable);
        }
        done += read;
    }
    Ok(bytes)
}

/// What the card's `paperpaw.ini` chooses.
///
/// # Errors
/// As [`read`].
pub(crate) fn card_choice(storage: &dyn Storage) -> Result<Choice, Failure> {
    // As the firmware reads it: a line that is no UTF-8 is skipped, not a
    // refusal of the file.
    Ok(choice_of(&read(storage)?))
}

/// Sets `dual-boot` in the card's `paperpaw.ini`: read whole, edited, written
/// whole.
///
/// # Errors
/// [`Failure`]; the file is as it was.
pub(crate) fn set(storage: &dyn Storage, mode: Mode) -> Result<(), Failure> {
    let bytes = read(storage)?;
    let text = core::str::from_utf8(&bytes).map_err(|_| Failure::Unreadable)?;
    let edited = edit(text, mode)?;
    storage.write(INI_PATH, &edited).map_err(|_| Failure::Write)
}

#[cfg(test)]
#[path = "dual_boot_ini_tests.rs"]
mod tests;
