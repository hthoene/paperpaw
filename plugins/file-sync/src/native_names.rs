//! Names: the card's paths of a pair, the name a conflicting copy is kept
//! under, the date it carries and the trash's names.
use alloc::string::String;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::{concat, copy, format};

/// The longest name of a file.
const NAME_MAX: usize = 250;

/// `YYYY-MM-DD` of a day number (days since 1970-01-01, civil calendar).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn date(days: i64) -> Result<String, Error> {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format(format_args!("{year:04}-{month:02}-{day:02}"))
}

/// The device's name as part of a file name: letters, digits, `-` and `_`,
/// at most 20; "device" where nothing is left.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn device(name: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(20).map_err(|_| Error::NoMemory)?;
    for c in name.chars() {
        if out.len() >= 20 {
            break;
        }
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c == ' ' && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        return copy("device");
    }
    copy(trimmed)
}

/// `name (conflict device date).ext` (`n` after 1 adds ` n` before the
/// bracket closes), the name cut to fit.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn conflict(name: &str, device: &str, date: &str, n: u32) -> Result<String, Error> {
    let (base, ext) = match name.rfind('.') {
        Some(at) if at > 0 => name.split_at(at),
        _ => (name, ""),
    };
    let tail = if n > 1 {
        format(format_args!(" (conflict {device} {date} {n})"))?
    } else {
        format(format_args!(" (conflict {device} {date})"))?
    };
    let room = NAME_MAX.saturating_sub(tail.len() + ext.len());
    let mut cut = base.len().min(room);
    while !base.is_char_boundary(cut) {
        cut -= 1;
    }
    concat(&[base.get(..cut).unwrap_or(""), &tail, ext])
}

/// The card's path of `name` in the folder `rel` of the pair's folder `root`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn path(root: &str, rel: &str, name: &str) -> Result<String, Error> {
    if rel.is_empty() {
        concat(&[root, "/", name])
    } else {
        concat(&[root, "/", rel, "/", name])
    }
}

/// The path of a folder `name` in `rel` (a folder of the pair, relative).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn child(rel: &str, name: &str) -> Result<String, Error> {
    if rel.is_empty() {
        copy(name)
    } else {
        concat(&[rel, "/", name])
    }
}

/// The order of two names as the card tells them apart: without case, as FAT
/// does (`Note.md` and `note.md` are one file of the card, and two of most
/// servers).
#[must_use]
pub fn order(a: &str, b: &str) -> core::cmp::Ordering {
    a.chars()
        .flat_map(char::to_uppercase)
        .cmp(b.chars().flat_map(char::to_uppercase))
}

/// Whether the card takes two names for one.
#[must_use]
pub fn same(a: &str, b: &str) -> bool {
    order(a, b).is_eq()
}

/// Whether a name is left out of a sync: hidden (a dot), a file the host is
/// writing (`.pp-staged`), one the card or a server cannot take back (a
/// control character, a tab or a line end of the state) or one a path cannot
/// hold.
#[must_use]
pub fn left_out(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with('~')
        || name.ends_with(paperpaw_sdk::native::storage::STAGED_SUFFIX)
        || name.len() > NAME_MAX
        || name.chars().any(|c| c.is_control())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    #[test]
    fn a_day_number_is_a_date() {
        assert_eq!(date(0).unwrap(), "1970-01-01");
        assert_eq!(date(20_453).unwrap(), "2025-12-31");
        assert_eq!(date(20_454).unwrap(), "2026-01-01");
        assert_eq!(date(20_554).unwrap(), "2026-04-11");
        assert_eq!(date(11_016).unwrap(), "2000-02-29");
    }

    #[test]
    fn a_conflict_keeps_the_extension_and_can_be_numbered() {
        assert_eq!(
            conflict("Note.md", "x4", "2026-11-18", 1).unwrap(),
            "Note (conflict x4 2026-11-18).md"
        );
        assert_eq!(
            conflict("Note.md", "x4", "2026-11-18", 3).unwrap(),
            "Note (conflict x4 2026-11-18 3).md"
        );
        assert_eq!(
            conflict("README", "x4", "d", 1).unwrap(),
            "README (conflict x4 d)"
        );
        assert_eq!(
            conflict(".hidden", "x4", "d", 1).unwrap(),
            ".hidden (conflict x4 d)"
        );
        let long = alloc::format!("{}.txt", "ä".repeat(200));
        assert!(conflict(&long, "x4", "2026-11-18", 1).unwrap().len() <= NAME_MAX);
    }

    #[test]
    fn the_device_name_is_made_safe() {
        assert_eq!(device("My X4: Hannes'").unwrap(), "My-X4-Hannes");
        assert_eq!(device("???").unwrap(), "device");
    }

    #[test]
    fn some_names_are_left_out() {
        for name in [".obsidian", "a.pp-staged", "~lock", "a\tb", "a\nb"] {
            assert!(left_out(name), "{name}");
        }
        assert!(!left_out("Note ü.md"));
    }

    #[test]
    fn names_are_told_apart_as_the_card_does() {
        assert!(same("Note \u{e4}.md", "NOTE \u{c4}.MD"));
        assert!(!same("Note.md", "Note.txt"));
        assert_eq!(order("a.md", "B.md"), core::cmp::Ordering::Less);
        assert_eq!(order("b.md", "A.md"), core::cmp::Ordering::Greater);
    }

    #[test]
    fn paths_are_joined() {
        assert_eq!(path("Notes", "", "a.md").unwrap(), "Notes/a.md");
        assert_eq!(path("Notes", "x/y", "a.md").unwrap(), "Notes/x/y/a.md");
        assert_eq!(child("", "x").unwrap(), "x");
        assert_eq!(child("x", "y").unwrap(), "x/y");
    }
}
