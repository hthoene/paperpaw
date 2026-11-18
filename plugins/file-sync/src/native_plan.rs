//! What one file needs, decided from where it is (the card, the server), what
//! it was at the last sync (the state) and the mode. Nothing here touches a
//! file or the network: the rules of "never lose data" are these and nothing
//! else.
//!
//! - Changed on one side only: the other side is brought up to date.
//! - Changed on both (or on both and never synced, with another size): keep
//!   both, the card's copy under another name.
//! - Gone on one side and unchanged on the other: gone on the other too (the
//!   card's copy goes to the trash). Gone on one and changed on the other:
//!   the change is kept.
//! - Without a state nothing is ever deleted.
use alloc::string::String;

/// How a pair is synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Changes go both ways.
    #[default]
    TwoWay,
    /// The server's changes come to the card; the card's go nowhere.
    DownloadOnly,
}

/// One side of a file: its size and what changes with it (the card's time, the
/// server's `ETag` or time as a number; 0 where unknown).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Side {
    pub size: u64,
    pub stamp: u64,
}

/// What was kept of a file at the last sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub name: String,
    pub local: Side,
    pub remote: Side,
}

/// What a file needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing: the sides agree with the state.
    Nothing,
    /// Both sides are there with one size and no state: taken to be the same.
    Adopt,
    /// The server's file replaces the card's (or comes to it).
    Download,
    /// The card's file replaces the server's (or goes to it).
    Upload,
    /// The card's copy goes to the trash.
    DeleteLocal,
    /// The server's file is deleted.
    DeleteRemote,
    /// Both changed: the card's copy is kept under another name, then the
    /// server's comes.
    Conflict,
    /// Only the state is dropped.
    Forget,
}

impl Side {
    /// Whether the card's file is not what it was at the last sync.
    #[must_use]
    pub fn local_changed(self, kept: &Kept) -> bool {
        self.size != kept.local.size || self.stamp != kept.local.stamp
    }

    /// Whether the server's file is not what it was at the last sync (the
    /// `ETag` counts only where both sides know one).
    #[must_use]
    pub fn remote_changed(self, kept: &Kept) -> bool {
        self.size != kept.remote.size
            || (self.stamp != 0 && kept.remote.stamp != 0 && self.stamp != kept.remote.stamp)
    }
}

/// What the file `local` (on the card), `remote` (on the server) and `kept`
/// (the state) needs.
#[must_use]
pub fn decide(
    mode: Mode,
    local: Option<Side>,
    remote: Option<Side>,
    kept: Option<&Kept>,
) -> Action {
    let two = mode == Mode::TwoWay;
    match (local, remote, kept) {
        (Some(l), Some(r), Some(k)) => match (l.local_changed(k), r.remote_changed(k)) {
            (false, false) => Action::Nothing,
            (true, false) if two => Action::Upload,
            (true, false) => Action::Nothing,
            (false, true) => Action::Download,
            (true, true) => Action::Conflict,
        },
        (Some(l), Some(r), None) if l.size == r.size => Action::Adopt,
        (Some(_), Some(_), None) => Action::Conflict,
        (Some(l), None, Some(k)) => {
            if !l.local_changed(k) {
                Action::DeleteLocal
            } else if two {
                Action::Upload
            } else {
                Action::Forget
            }
        }
        (Some(_), None, None) if two => Action::Upload,
        (Some(_), None, None) => Action::Nothing,
        (None, Some(r), Some(k)) => {
            if two && !r.remote_changed(k) {
                Action::DeleteRemote
            } else {
                Action::Download
            }
        }
        (None, Some(_), None) => Action::Download,
        (None, None, Some(_)) => Action::Forget,
        (None, None, None) => Action::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(size: u64, stamp: u64) -> Side {
        Side { size, stamp }
    }

    fn kept() -> Kept {
        Kept {
            name: "a.md".into(),
            local: side(10, 100),
            remote: side(10, 7),
        }
    }

    const SAME_L: Side = Side {
        size: 10,
        stamp: 100,
    };
    const SAME_R: Side = Side { size: 10, stamp: 7 };
    const NEW_L: Side = Side {
        size: 12,
        stamp: 200,
    };
    const NEW_R: Side = Side { size: 11, stamp: 8 };

    fn d(mode: Mode, l: Option<Side>, r: Option<Side>, k: bool) -> Action {
        decide(mode, l, r, k.then(kept).as_ref())
    }

    #[test]
    fn a_change_on_one_side_goes_to_the_other() {
        let two = Mode::TwoWay;
        assert_eq!(d(two, Some(SAME_L), Some(SAME_R), true), Action::Nothing);
        assert_eq!(d(two, Some(NEW_L), Some(SAME_R), true), Action::Upload);
        assert_eq!(d(two, Some(SAME_L), Some(NEW_R), true), Action::Download);
        // A new file on one side only.
        assert_eq!(d(two, Some(NEW_L), None, false), Action::Upload);
        assert_eq!(d(two, None, Some(NEW_R), false), Action::Download);
    }

    #[test]
    fn a_change_on_both_sides_keeps_both() {
        for mode in [Mode::TwoWay, Mode::DownloadOnly] {
            assert_eq!(d(mode, Some(NEW_L), Some(NEW_R), true), Action::Conflict);
            // Never synced and not the same size: both are kept.
            assert_eq!(d(mode, Some(NEW_L), Some(NEW_R), false), Action::Conflict);
        }
        // Never synced and one size: the same file, taken as such.
        assert_eq!(
            d(Mode::TwoWay, Some(side(10, 5)), Some(side(10, 9)), false),
            Action::Adopt
        );
    }

    #[test]
    fn the_etag_alone_marks_a_change_of_the_server_where_the_size_is_the_same() {
        let r = side(10, 99);
        assert_eq!(
            d(Mode::TwoWay, Some(SAME_L), Some(r), true),
            Action::Download
        );
        // An unknown ETag on either side: the size decides.
        let unknown = Kept {
            remote: side(10, 0),
            ..kept()
        };
        assert_eq!(
            decide(Mode::TwoWay, Some(SAME_L), Some(r), Some(&unknown)),
            Action::Nothing
        );
    }

    #[test]
    fn a_file_gone_on_one_side_goes_on_the_other_only_where_it_did_not_change() {
        let two = Mode::TwoWay;
        // Deleted on the server, unchanged on the card: the card's copy goes.
        assert_eq!(d(two, Some(SAME_L), None, true), Action::DeleteLocal);
        // Deleted on the server, changed on the card: the change is kept.
        assert_eq!(d(two, Some(NEW_L), None, true), Action::Upload);
        // Deleted on the card, unchanged on the server: deleted there.
        assert_eq!(d(two, None, Some(SAME_R), true), Action::DeleteRemote);
        // Deleted on the card, changed on the server: the change comes back.
        assert_eq!(d(two, None, Some(NEW_R), true), Action::Download);
        // Gone on both.
        assert_eq!(d(two, None, None, true), Action::Forget);
    }

    #[test]
    fn download_only_never_changes_the_server_and_keeps_what_is_changed_on_the_card() {
        let one = Mode::DownloadOnly;
        assert_eq!(d(one, Some(NEW_L), Some(SAME_R), true), Action::Nothing);
        assert_eq!(d(one, Some(NEW_L), None, false), Action::Nothing);
        assert_eq!(d(one, Some(SAME_L), None, true), Action::DeleteLocal);
        assert_eq!(d(one, Some(NEW_L), None, true), Action::Forget);
        // What was deleted on the card comes again: the card mirrors the server.
        assert_eq!(d(one, None, Some(SAME_R), true), Action::Download);
        assert_eq!(d(one, None, Some(NEW_R), false), Action::Download);
    }

    #[test]
    fn without_a_state_nothing_is_deleted() {
        for mode in [Mode::TwoWay, Mode::DownloadOnly] {
            for action in [
                d(mode, Some(NEW_L), None, false),
                d(mode, None, Some(NEW_R), false),
                d(mode, Some(NEW_L), Some(NEW_R), false),
            ] {
                assert!(!matches!(
                    action,
                    Action::DeleteLocal | Action::DeleteRemote
                ));
            }
        }
    }
}
