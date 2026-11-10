//! The trust rules of `docs/abi/device.md` 5.2 for installing a plugin,
//! applied to what the check of the staged copy found and to the evidence
//! the asking plugin sent (`install_author`, `install_rotation`). The store's
//! `trust.rs` rules, moved into the host and made a pure decision: nothing is
//! written here; the pins to take are returned and taken at commit
//! (`plugin_swap`).
//!
//! * A code plugin only: a data plugin has no author to pin.
//! * A digest other than the evidence's `sha256` is reason 18.
//! * The pinned key revoked for the id: reason 7.
//! * The id is pinned to the signing key: it installs.
//! * The id is pinned to another key: only rotations lead from it to the
//!   signing key, each signed by the pinned recovery key (steps before the
//!   pinned key are skipped, as the store's `follow`; one after the first
//!   taken that does not hold refuses); else reason 6.
//! * A new id: `install_author.author` is the signing key (pinned, with its
//!   recovery key), else 5. No `install_author` at all is a sideload: the
//!   user confirms the key's fingerprint, and the key is pinned.
use paperpaw_native::signature::{rotation_message, verify_message};

use super::plugin_job::reason;
use super::plugin_trust::Standing;

/// Most rotation statements of one request (`device.md` 3, the batch limit).
pub(crate) const ROTATIONS_MAX: usize = 6;

/// One rotation: the catalog's statement that the plugin moves to key `to`,
/// signed by the recovery key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rotation {
    pub(crate) to: [u8; 32],
    pub(crate) signature: [u8; 64],
}

/// What the catalog says (`install_author`): each key 0 or 32 bytes on the
/// wire, `None` for none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Author {
    pub(crate) author: Option<[u8; 32]>,
    /// Named for a first install only.
    pub(crate) recovery: Option<[u8; 32]>,
    /// The code's expected digest.
    pub(crate) sha256: Option<[u8; 32]>,
}

/// The evidence of one request: the author record and its rotations, in the
/// author's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Evidence {
    /// `None` where the plugin sent no `install_author`: a sideload.
    pub(crate) author: Option<Author>,
    rotations: [Rotation; ROTATIONS_MAX],
    count: u8,
}

impl Evidence {
    /// No evidence: a sideload.
    pub(crate) const NONE: Self = Self {
        author: None,
        rotations: [Rotation {
            to: [0; 32],
            signature: [0; 64],
        }; ROTATIONS_MAX],
        count: 0,
    };

    pub(crate) fn new(author: Option<Author>, rotations: &[Rotation]) -> Option<Self> {
        let mut evidence = Self {
            author,
            ..Self::NONE
        };
        evidence
            .rotations
            .get_mut(..rotations.len())?
            .copy_from_slice(rotations);
        evidence.count = rotations.len() as u8;
        Some(evidence)
    }

    pub(crate) fn rotations(&self) -> &[Rotation] {
        &self.rotations[..usize::from(self.count)]
    }

    /// Adds the next rotation statement; `false` past [`ROTATIONS_MAX`].
    pub(crate) fn push_rotation(&mut self, step: Rotation) -> bool {
        let at = usize::from(self.count);
        let Some(slot) = self.rotations.get_mut(at) else {
            return false;
        };
        *slot = step;
        self.count += 1;
        true
    }
}

/// What the check of the staged copy found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Staged<'a> {
    pub(crate) id: &'a str,
    /// The signing key and the code's digest; `None` for a data plugin.
    pub(crate) code: Option<([u8; 32], [u8; 32])>,
    pub(crate) standing: Standing,
}

/// The pins an install takes at its commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pins {
    /// At a first install: the author's key and, if named, its recovery key.
    pub(crate) first: Option<([u8; 32], Option<[u8; 32]>)>,
    /// The rotations that move the pinned key to the signing key, in order.
    rotations: [Rotation; ROTATIONS_MAX],
    count: u8,
    /// The code's digest, named as installed once its key is pinned.
    pub(crate) code: Option<[u8; 32]>,
}

impl Pins {
    pub(crate) const NONE: Self = Self {
        first: None,
        rotations: Evidence::NONE.rotations,
        count: 0,
        code: None,
    };

    pub(crate) fn rotations(&self) -> &[Rotation] {
        &self.rotations[..usize::from(self.count)]
    }

    /// Pins with `rotations` (at most [`ROTATIONS_MAX`]).
    pub(crate) fn with(
        first: Option<([u8; 32], Option<[u8; 32]>)>,
        rotations: &[Rotation],
        code: Option<[u8; 32]>,
    ) -> Option<Self> {
        let mut pins = Self {
            first,
            code,
            ..Self::NONE
        };
        pins.rotations
            .get_mut(..rotations.len())?
            .copy_from_slice(rotations);
        pins.count = rotations.len() as u8;
        Some(pins)
    }
}

/// What the rules decide.
// One short-lived value per request, passed once: boxing the pins would only
// add an allocation that can fail.
#[allow(
    clippy::large_enum_variant,
    reason = "passed once and consumed at once; boxing would add an allocation that can fail"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Install {
        /// No evidence for a new id: the user confirms the key's fingerprint,
        /// "not from a catalog", and a trusted-installer grant never skips it.
        sideload: bool,
        pins: Pins,
    },
    /// Refused with this reason (5.5).
    Refused(u8),
}

/// Applies the rules of 5.2.
#[inline(never)]
pub(crate) fn decide(staged: &Staged<'_>, evidence: &Evidence) -> Decision {
    let expected = evidence.author.and_then(|author| author.sha256);
    let Some((key, digest)) = staged.code else {
        // A data plugin: no code to bind, no author to pin. A digest named
        // for code that is not there is a content that changed.
        return if expected.is_some() {
            Decision::Refused(reason::CHANGED)
        } else {
            Decision::Install {
                sideload: false,
                pins: Pins::NONE,
            }
        };
    };
    if expected.is_some_and(|expected| expected != digest) {
        return Decision::Refused(reason::CHANGED);
    }
    let code = Some(digest);
    match staged.standing.pinned {
        Some(pinned) if pinned == key => {
            if staged.standing.revoked {
                return Decision::Refused(reason::REVOKED);
            }
            install(false, None, &[], code)
        }
        Some(pinned) => match follow(staged, evidence, pinned, key) {
            Some(steps) => install(false, None, steps.as_slice(), code),
            None => Decision::Refused(reason::KEY_CHANGED),
        },
        None => match evidence.author {
            Some(author) if author.author == Some(key) => {
                install(false, Some((key, author.recovery)), &[], code)
            }
            Some(_) => Decision::Refused(reason::NOT_THE_AUTHOR),
            // A sideload: the user confirms the key; it is pinned alone.
            None => install(true, Some((key, None)), &[], code),
        },
    }
}

#[inline(never)]
fn install(
    sideload: bool,
    first: Option<([u8; 32], Option<[u8; 32]>)>,
    rotations: &[Rotation],
    code: Option<[u8; 32]>,
) -> Decision {
    match Pins::with(first, rotations, code) {
        Some(pins) => Decision::Install { sideload, pins },
        None => Decision::Refused(reason::DAMAGED),
    }
}

/// The steps taken.
struct Steps {
    list: [Rotation; ROTATIONS_MAX],
    count: usize,
}

impl Steps {
    fn as_slice(&self) -> &[Rotation] {
        &self.list[..self.count]
    }
}

/// Takes the author from `pinned` along the evidence's rotations to `key`:
/// the steps up to the one that names `key`, those before the pinned key
/// skipped, none refused after the first taken. `None` where it does not
/// lead there (or no recovery key is pinned).
#[inline(never)]
fn follow(
    staged: &Staged<'_>,
    evidence: &Evidence,
    pinned: [u8; 32],
    key: [u8; 32],
) -> Option<Steps> {
    let recovery = staged.standing.recovery?;
    let steps = evidence.rotations();
    let last = steps.iter().position(|step| step.to == key)?;
    let mut taken = Steps {
        list: Evidence::NONE.rotations,
        count: 0,
    };
    let mut current = pinned;
    for step in &steps[..=last] {
        let holds = step.to != current
            && verify_message(
                &recovery,
                rotation_message(staged.id, &current, &step.to).as_bytes(),
                &step.signature,
            );
        if holds {
            *taken.list.get_mut(taken.count)? = *step;
            taken.count += 1;
            current = step.to;
        } else if taken.count > 0 {
            // After the first taken: a step that does not hold.
            return None;
        }
    }
    (current == key).then_some(taken)
}

#[cfg(test)]
#[path = "plugin_rules_tests.rs"]
mod tests;
