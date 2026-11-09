//! What the user decided about the network, as `core.net` and `core.sockets`
//! see it (`network.md` 3.2): the device's power mode and each plugin's own
//! setting (its network off, or held back in saving). Core decides; the shell
//! is told nothing of the policy and asked nothing about it.
//!
//! The kernel keeps a snapshot here ([`NetPolicy::update`], at the end of
//! every call) and the two services read it: before a request or socket
//! starts, and again on every tick for the ones that run, so that turning a
//! plugin's network off ends what it has open at the next pass.
//!
//! The snapshot is rebuilt only when the settings changed (a version) or the
//! mode did, so an ordinary call costs three comparisons. Every allocation is
//! fallible; when the snapshot cannot be made the policy is *closed*: every
//! plugin's network is off until it can be made (a privacy setting is never
//! guessed open).
use super::net_service::{PowerEnd, reason};
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;

#[derive(Default)]
struct Snapshot {
    /// The version of the kernel's settings this was made from.
    version: u32,
    /// The device is offline (the user's mode, unless the reader just went
    /// online).
    offline: bool,
    /// The mode itself is offline: a plugin held back in saving is not held
    /// back further.
    mode_offline: bool,
    /// Plugins whose network the user turned off.
    off: Vec<String>,
    /// Plugins whose network use the user put on hold (saving).
    held: Vec<String>,
    /// The snapshot could not be made: nothing goes out.
    closed: bool,
}

/// The device's network policy (see the module).
#[derive(Default)]
pub(crate) struct NetPolicy {
    snapshot: RefCell<Snapshot>,
}

/// Copies `names` into a vector, or `None` when there is no memory.
fn copy_all<'a>(names: impl Iterator<Item = &'a str>) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for name in names {
        super::net_service::gate().ok()?;
        out.try_reserve(1).ok()?;
        let mut owned = String::new();
        owned.try_reserve_exact(name.len()).ok()?;
        owned.push_str(name);
        out.push(owned);
    }
    Some(out)
}

impl NetPolicy {
    /// An open policy: nothing is off, held or offline until the kernel says.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the snapshot is out of date for these inputs: no allocation.
    pub(crate) fn needs(&self, version: u32, offline: bool, mode_offline: bool) -> bool {
        let Ok(s) = self.snapshot.try_borrow() else {
            return true;
        };
        s.closed || s.version != version || s.offline != offline || s.mode_offline != mode_offline
    }

    /// Makes the snapshot: `off` are the plugins whose network is off, `held`
    /// those held back in saving. When it cannot be made, the policy closes
    /// (and [`NetPolicy::needs`] stays true, so the next pass tries again).
    pub(crate) fn update<'a>(
        &self,
        version: u32,
        offline: bool,
        mode_offline: bool,
        off: impl Iterator<Item = &'a str>,
        held: impl Iterator<Item = &'a str>,
    ) {
        let Ok(mut snapshot) = self.snapshot.try_borrow_mut() else {
            return;
        };
        let lists = copy_all(off).and_then(|off| Some((off, copy_all(held)?)));
        match lists {
            Some((off, held)) => {
                *snapshot = Snapshot {
                    version,
                    offline,
                    mode_offline,
                    off,
                    held,
                    closed: false,
                };
            }
            None => {
                // Nothing is allocated to close: the old lists are dropped by
                // the flag, not by a copy.
                snapshot.closed = true;
            }
        }
    }

    /// Whether `plugin`'s network use ends now, and how (`network.md` 3.2),
    /// without any allocation. `background`: for a run of the plugin's work,
    /// the only use a hold in saving refuses (the reader asking is the reader
    /// asking).
    ///
    /// The user's *off* comes first, then the offline mode, then the hold.
    pub(crate) fn end_for(&self, plugin: &str, background: bool) -> Option<PowerEnd> {
        // A snapshot being rebuilt is never read inside a call; if it is, the
        // answer is the safe one.
        let Ok(s) = self.snapshot.try_borrow() else {
            return Some(PowerEnd {
                reason: reason::NETWORK_OFF,
                later: false,
            });
        };
        if s.closed || s.off.iter().any(|p| p == plugin) {
            return Some(PowerEnd {
                reason: reason::NETWORK_OFF,
                later: false,
            });
        }
        if s.offline {
            return Some(PowerEnd {
                reason: reason::OFFLINE,
                later: true,
            });
        }
        if background && !s.mode_offline && s.held.iter().any(|p| p == plugin) {
            return Some(PowerEnd {
                reason: reason::SAVING,
                later: true,
            });
        }
        None
    }
}

#[cfg(test)]
#[path = "net_policy_tests.rs"]
mod tests;
