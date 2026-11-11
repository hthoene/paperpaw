//! Why an install, update or check ended, in words (slice 6, host-free): the
//! host's outcome reasons (`docs/abi/device.md` 5.5, one list for
//! `install_done.reason`, `dev_check.reason` and `dev_install.reason`) mapped to
//! the store's texts (en/de in `i18n`). The interface-6 `Refusal` of `trust.rs`
//! and `InstallFailure` map onto the reasons one to one; the host does the
//! checks, the plugin only says what they found.

/// What the store says of an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Installed (reason 0).
    Done,
    /// The reader's own word or the host's stop: nothing to say (reasons 2, 3,
    /// 20).
    Silent,
    /// A text with the plugin's name (`{name}`): the key.
    Said(&'static str),
}

/// The store's words for the host's outcome `reason` (a value this list does not
/// know reads as 1, "other"). `flash_hint`: the device runs the stock layout of
/// the code flash, where the full layout gives much more room, so *no room* says
/// so.
#[must_use]
pub const fn outcome(reason: u32, flash_hint: bool) -> Outcome {
    match reason {
        0 => Outcome::Done,
        2 | 3 | 20 => Outcome::Silent,
        4 => Outcome::Said("store.refused.damaged"),
        5 => Outcome::Said("store.refused.not-the-author"),
        6 => Outcome::Said("store.refused.key-changed"),
        7 => Outcome::Said("store.refused.revoked"),
        8 => Outcome::Said("store.refused.unsigned"),
        9 => Outcome::Said("store.refused.unknown-key"),
        10 => Outcome::Said("store.refused.needs-newer"),
        11 => Outcome::Said("store.refused.older"),
        12 => Outcome::Said("store.refused.other-build"),
        13 => Outcome::Said(if flash_hint {
            "store.refused.no-room-flash"
        } else {
            "store.refused.no-room"
        }),
        14 => Outcome::Said("store.refused.same-version"),
        15 => Outcome::Said("store.refused.taken"),
        16 => Outcome::Said("store.refused.card"),
        17 => Outcome::Said("store.refused.card-full"),
        18 => Outcome::Said("store.refused.changed"),
        19 => Outcome::Said("store.refused.unsupported"),
        21 => Outcome::Said("store.refused.restart-first"),
        _ => Outcome::Said("store.refused.other"),
    }
}

/// Whether the code plugin installed starts only after a restart
/// (`install_done.flags` bit 1): the notice says so.
#[must_use]
pub const fn after_restart(flags: u32) -> bool {
    flags & 2 != 0
}

/// Whether the plugin installed is a data plugin (`flags` bit 0 of a check).
#[must_use]
pub const fn is_data(flags: u32) -> bool {
    flags & 1 != 0
}
