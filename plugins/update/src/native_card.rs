//! What the native update page decides without the host: which files of the
//! card's `updates` folder it offers, how an install's end reads, which text
//! says why it failed, how far it is, and the two lines of `installing.txt`
//! (the same file as interface 6 writes, `docs/data-files.md`). The device
//! checks a file and asks the reader before it installs (decisions 132 and
//! 134), so this knows nothing of versions or signers.
use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;
use paperpaw_sdk::native::{Error, device::DeviceOutcome};
use paperpaw_ui_kit::{mem::push, text::concat};

/// The folder of the card the update files are in.
pub const UPDATE_DIR: &str = "updates";
/// Files offered at most (a card with more is cut short, in the order the card
/// lists them).
pub const MOST_FILES: usize = 8;
/// The longest file name offered.
pub const MAX_NAME: usize = 64;
/// The caller's own number of the install request, the one this page sends.
pub const REQUEST: u32 = 1;
/// The number of the check of the first file of `updates`; the next file's is
/// one more (at most `MOST_FILES`, one check at a time).
pub const CHECK_REQUEST: u32 = 2;
/// The longest line of `installing.txt` read: a path in `updates`.
pub const MAX_PAIR: usize = 4096;

/// Whether `name` is an update file: a file ending in `.bin`, not hidden, a
/// name that fits.
#[must_use]
pub fn is_update_file(name: &str, folder: bool, unaddressable: bool) -> bool {
    !folder
        && !unaddressable
        && !name.starts_with('.')
        && name.len() <= MAX_NAME
        && paperpaw_sdk::files::has_extension(name, "bin")
}

/// The files to offer, in the order they were added.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Offered {
    names: Vec<String>,
}

impl Offered {
    /// Adds a file of the folder if it is an update file and there is room.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the name cannot be kept.
    pub fn add(&mut self, name: &str, folder: bool, unaddressable: bool) -> Result<(), Error> {
        if self.names.len() < MOST_FILES && is_update_file(name, folder, unaddressable) {
            push(&mut self.names, paperpaw_ui_kit::text::copy(name)?)?;
        }
        Ok(())
    }

    /// The names, sorted as numbers inside them count (`0.9.0` before `0.10.0`).
    #[must_use]
    pub fn finish(mut self) -> Vec<String> {
        paperpaw_sdk::sort::sort_by(&mut self.names, |a, b| natural(a, b));
        self.names
    }
}

/// `a` against `b`, digits read as numbers, letters without regard to case.
pub(crate) fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.as_bytes(), b.as_bytes());
    loop {
        let order = match (x.first(), y.first()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let (one, rest_x) = number(x);
                let (two, rest_y) = number(y);
                x = rest_x;
                y = rest_y;
                one.len().cmp(&two.len()).then_with(|| one.cmp(two))
            }
            (Some(p), Some(q)) => {
                x = x.get(1..).unwrap_or_default();
                y = y.get(1..).unwrap_or_default();
                p.to_ascii_lowercase().cmp(&q.to_ascii_lowercase())
            }
        };
        if order != Ordering::Equal {
            return order;
        }
    }
}

/// The digits at the start of `text` without their leading zeros, and the rest.
fn number(text: &[u8]) -> (&[u8], &[u8]) {
    let end = text
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, rest) = text.split_at(end);
    let start = digits
        .iter()
        .position(|b| *b != b'0')
        .unwrap_or(digits.len());
    (digits.get(start..).unwrap_or_default(), rest)
}

/// How an install request ended, as the page treats it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Written: the restart starts it.
    Done,
    /// The reader said no, or the confirmation timed out: nothing happened.
    Refused,
    /// The device says it is not newer than the version running, though the
    /// check and the reader's choice allowed it.
    NotNewer,
    /// An installed image waits for its restart.
    Pending,
    /// Failed for this reason.
    Failed(DeviceOutcome),
}

/// What `status` and the verdict of `install_done` come to.
#[must_use]
pub fn ended(status_ok: bool, outcome: DeviceOutcome) -> Ended {
    match outcome {
        DeviceOutcome::Ok if status_ok => Ended::Done,
        DeviceOutcome::Declined | DeviceOutcome::Dismissed => Ended::Refused,
        DeviceOutcome::NotNewer => Ended::NotNewer,
        DeviceOutcome::RestartPending => Ended::Pending,
        DeviceOutcome::Ok => Ended::Failed(DeviceOutcome::Other),
        other => Ended::Failed(other),
    }
}

/// The text that says why an install failed.
#[must_use]
pub const fn reason(outcome: DeviceOutcome) -> &'static str {
    match outcome {
        DeviceOutcome::Damaged | DeviceOutcome::ContentChanged => "update.reason.damaged",
        DeviceOutcome::OtherAuthor
        | DeviceOutcome::KeyChanged
        | DeviceOutcome::Revoked
        | DeviceOutcome::Unsigned
        | DeviceOutcome::UnknownKey => "update.reason.untrusted",
        DeviceOutcome::NewerInterface => "update.reason.newer-format",
        DeviceOutcome::OtherChip => "update.reason.other-device",
        DeviceOutcome::NoRoom => "update.reason.too-large",
        DeviceOutcome::NotNewer => "update.reason.not-newer",
        DeviceOutcome::CardError => "update.reason.unreadable",
        DeviceOutcome::CardFull => "update.reason.write",
        DeviceOutcome::Unsupported => "update.reason.unsupported",
        _ => "update.reason.other",
    }
}

/// How far an install is, in whole percent, where the total is known.
#[must_use]
pub fn percent(done: u64, total: u64) -> Option<i64> {
    if total == 0 {
        return None;
    }
    // The device's own count never passes the total; a count that does is a
    // full bar, not more.
    let done = done.min(total);
    i64::try_from(u128::from(done) * 100 / u128::from(total)).ok()
}

/// The path of the update file `name` of the folder.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn path(name: &str) -> Result<String, Error> {
    concat(&[UPDATE_DIR, "/", name])
}

/// The content of `installing.txt`: the update's path and its version (the
/// device's check said it), a line each.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn pair(path: &str, version: &str) -> Result<String, Error> {
    concat(&[path, "\n", version, "\n"])
}

/// The path and the version of `installing.txt`.
#[must_use]
pub fn read_pair(bytes: &[u8]) -> Option<(&str, &str)> {
    let text = core::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    Some((lines.next()?, lines.next()?))
}

/// Whether the file of `installing.txt` is one this plugin removes after the
/// update: a name in `updates`, nothing else (a data file is not trusted to
/// name any file of the card).
#[must_use]
pub fn is_update_path(path: &str) -> bool {
    path.strip_prefix(UPDATE_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| !name.contains('/') && is_update_file(name, false, false))
}

/// Whether a start after an install says it went well: the firmware did not
/// undo it, and the version `installing.txt` names (where it names one) is the
/// one running.
#[must_use]
pub fn came(undone: bool, named: &str, running: &str) -> bool {
    !undone && (named.is_empty() || named == running)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[(&str, bool, bool)]) -> Vec<String> {
        let mut offered = Offered::default();
        for (name, folder, unaddressable) in list {
            offered.add(name, *folder, *unaddressable).unwrap();
        }
        offered.finish()
    }

    #[test]
    fn update_files_end_in_bin_and_are_neither_hidden_nor_folders() {
        let found = names(&[
            ("paperpaw-x4-0.10.0.bin", false, false),
            ("paperpaw-x4-0.9.0.BIN", false, false),
            ("notes.txt", false, false),
            (".hidden.bin", false, false),
            ("folder.bin", true, false),
            ("odd.bin", false, true),
            ("bin", false, false),
        ]);
        assert_eq!(found, ["paperpaw-x4-0.9.0.BIN", "paperpaw-x4-0.10.0.bin"]);
        assert_eq!(natural("a007.bin", "a7.bin"), "a007.bin".cmp("a7.bin"));
        assert_eq!(natural("A2.bin", "a10.bin"), Ordering::Less);
    }

    #[test]
    fn a_card_with_many_files_is_cut_short_and_long_names_are_left_out() {
        let mut offered = Offered::default();
        for number in 0..20 {
            let name = alloc::format!("u{number}.bin");
            offered.add(&name, false, false).unwrap();
        }
        assert_eq!(offered.finish().len(), MOST_FILES);
        assert!(!is_update_file(
            &alloc::format!("{}.bin", "x".repeat(70)),
            false,
            false
        ));
    }

    #[test]
    fn an_end_reads_as_what_the_page_does_next() {
        use DeviceOutcome as O;
        assert_eq!(ended(true, O::Ok), Ended::Done);
        assert_eq!(ended(false, O::Ok), Ended::Failed(O::Other));
        assert_eq!(ended(false, O::Declined), Ended::Refused);
        assert_eq!(ended(false, O::Dismissed), Ended::Refused);
        assert_eq!(ended(false, O::NotNewer), Ended::NotNewer);
        assert_eq!(ended(false, O::RestartPending), Ended::Pending);
        assert_eq!(ended(false, O::Damaged), Ended::Failed(O::Damaged));
        // A refusal whose status says nothing went well is not a success.
        assert_eq!(ended(true, O::UnknownKey), Ended::Failed(O::UnknownKey));
    }

    #[test]
    fn every_failure_has_a_text() {
        use DeviceOutcome as O;
        for outcome in [
            O::Other,
            O::Damaged,
            O::OtherAuthor,
            O::KeyChanged,
            O::Revoked,
            O::Unsigned,
            O::UnknownKey,
            O::NewerInterface,
            O::OlderInterface,
            O::OtherChip,
            O::NoRoom,
            O::NotNewer,
            O::IdTaken,
            O::CardError,
            O::CardFull,
            O::ContentChanged,
            O::Unsupported,
            O::Cancelled,
        ] {
            assert!(reason(outcome).starts_with("update.reason."), "{outcome:?}");
        }
        assert_eq!(reason(O::UnknownKey), "update.reason.untrusted");
    }

    #[test]
    fn progress_is_whole_percent_and_never_past_the_end() {
        assert_eq!(percent(0, 0), None);
        assert_eq!(percent(0, 200), Some(0));
        assert_eq!(percent(50, 200), Some(25));
        assert_eq!(percent(300, 200), Some(100));
        assert_eq!(percent(u64::MAX, u64::MAX), Some(100));
    }

    #[test]
    fn the_pair_is_the_file_interface_6_wrote() {
        let text = include_str!("../testdata/installing.txt.v1");
        assert_eq!(
            read_pair(text.as_bytes()),
            Some(("updates/paperpaw-x4-0.4.0.bin", "0.4.0"))
        );
        assert_eq!(
            pair("updates/a.bin", "0.4.0").unwrap(),
            "updates/a.bin\n0.4.0\n"
        );
        // Without a version the second line is empty (an older file); both read it.
        let empty = pair("updates/a.bin", "").unwrap();
        assert_eq!(read_pair(empty.as_bytes()), Some(("updates/a.bin", "")));
        assert_eq!(read_pair(b"only one line"), None);
        assert_eq!(read_pair(&[0xff, 0xfe]), None);
    }

    #[test]
    fn the_offered_update_of_version_1_is_the_pair_written_now() {
        // `offered.txt` is compared as text: an update offered before is not
        // offered again.
        let kept = include_str!("../testdata/offered.txt.v1");
        assert_eq!(
            pair("updates/paperpaw-x4-0.5.0.bin", "0.5.0").unwrap(),
            kept
        );
        assert_eq!(
            read_pair(kept.as_bytes()),
            Some(("updates/paperpaw-x4-0.5.0.bin", "0.5.0"))
        );
    }

    #[test]
    fn only_a_file_in_updates_is_removed_after_an_update() {
        assert!(is_update_path("updates/paperpaw-x4-0.4.0.bin"));
        assert!(!is_update_path("updates/../books/a.bin"));
        assert!(!is_update_path("updates/sub/a.bin"));
        assert!(!is_update_path("books/a.bin"));
        assert!(!is_update_path("updates/a.txt"));
        assert!(!is_update_path("updates/"));
        assert!(!is_update_path("updates"));
    }

    #[test]
    fn a_start_says_it_went_well_unless_undone_or_another_version_runs() {
        assert!(came(false, "0.4.0", "0.4.0"));
        assert!(came(false, "", "0.4.0"));
        assert!(!came(true, "0.4.0", "0.4.0"));
        assert!(!came(false, "0.5.0", "0.4.0"));
    }
}
