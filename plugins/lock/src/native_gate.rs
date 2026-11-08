//! What PaperPaw starting, the device waking and *Lock now* do, and what they
//! never fail to do: the lock comes first, fail-closed. Whatever else is done
//! at those times (putting the kind back, the tries kept on the card) is done
//! best effort after it and its refusal is reported once the device is locked.
//!
//! An error returned from a callback drops every command it staged
//! (`screens-input.md` 2.1), the staged opening of the lock screen with them:
//! so once the opening is staged the callback returns `Ok`, and what failed
//! after it is dropped (the kind is put back at the next start or wake, the
//! tries are read again then). Only a callback that staged no opening reports
//! a refusal.
//!
//! The decision is made on the code kept alone: a code kept whole locks the
//! device whatever the kind setting says (the kind is put back with the code,
//! but a command takes effect after the callback, so this callback cannot
//! read the repaired value). The host is reached through [`Device`] so that
//! each of these paths is tested without one.
use paperpaw_sdk::native::Error;

use crate::native_flow::Lockstate;
use crate::native_model::{self as model, Code};

/// The kind of the lock in use: the kind of the code kept, never the kind
/// setting (a reset or a plain list of choices sets that to `off` while the
/// code stays, and a lock that a setting can switch off is none).
#[must_use]
pub fn active(stored: Option<&str>) -> Option<&'static str> {
    model::kept_kind(stored)
}

/// What the gate asks of the device.
pub trait Device {
    /// The code kept.
    ///
    /// # Errors
    /// `NoMemory`, or the host's refusal.
    fn stored(&self) -> Result<Option<Code>, Error>;
    /// Puts the kind back where it does not go with the code `stored`.
    ///
    /// # Errors
    /// `NoMemory`, or the host's refusal.
    fn repair_kind(&self, stored: Option<&str>) -> Result<(), Error>;
    /// The wrong tries in a row the card kept.
    ///
    /// # Errors
    /// The host's refusal.
    fn tries(&self) -> Result<u8, Error>;
    /// Whether waking up asks for the lock.
    ///
    /// # Errors
    /// The host's refusal.
    fn on_wake(&self) -> Result<bool, Error>;
    /// Opens the lock screen to unlock.
    ///
    /// # Errors
    /// `NoMemory`, or the host's refusal.
    fn open_unlock(&self) -> Result<(), Error>;
}

/// What the plugin knows of the lock screen it asked for.
#[derive(Debug, Default, Clone, Copy)]
pub struct Door {
    /// The screen was asked for and has not shown up (or closed) yet.
    pub opening: bool,
    /// An unlock screen is open.
    pub shown: bool,
}

/// Locks, if a code is kept whole and the device is not locked yet; locked
/// without a screen to unlock (it was refused, or closed and not reopened), it
/// opens one. The device counts as locked before the screen is asked for, so
/// that a refusal cannot leave it open.
///
/// # Errors
/// What opening the screen refused; the device is locked all the same.
pub fn lock_now(device: &impl Device, lock: &mut Lockstate, door: &mut Door) -> Result<(), Error> {
    let stored = device.stored()?;
    lock_kept(
        device,
        lock,
        door,
        model::kept_kind(stored.as_deref()).is_some(),
    )
}

fn lock_kept(
    device: &impl Device,
    lock: &mut Lockstate,
    door: &mut Door,
    kept: bool,
) -> Result<(), Error> {
    if !kept {
        return Ok(());
    }
    lock.locked = true;
    reopen(device, lock, door)
}

/// A locked device with no unlock screen gets one.
///
/// # Errors
/// What opening the screen refused; it is tried again at the next occasion.
pub fn reopen(device: &impl Device, lock: &Lockstate, door: &mut Door) -> Result<(), Error> {
    if !lock.locked || door.shown || door.opening {
        return Ok(());
    }
    device.open_unlock()?;
    door.opening = true;
    Ok(())
}

/// PaperPaw started: the device locks, then a wait due when it stopped is
/// waited anew and the kind goes back with the code. A refusal of either is
/// reported after the lock.
///
/// # Errors
/// The first refusal, once everything that could be done was.
pub fn started(
    device: &impl Device,
    lock: &mut Lockstate,
    door: &mut Door,
    now_ms: u64,
) -> Result<(), Error> {
    // Nothing asked for before this is still asked for.
    door.opening = false;
    let stored = device.stored();
    let (asked, locked) = locked(device, lock, door, &stored, true);
    let repaired = repair(device, &stored);
    let tries = device
        .tries()
        .map(|tries| lock.attempts.restore(tries, now_ms));
    settle(asked, locked.and(repaired).and(tries))
}

/// The lock part of a callback: whether the screen was asked for, and what
/// failed.
fn locked(
    device: &impl Device,
    lock: &mut Lockstate,
    door: &mut Door,
    stored: &Result<Option<Code>, Error>,
    wanted: bool,
) -> (bool, Result<(), Error>) {
    match stored {
        Ok(stored) if wanted => {
            match lock_kept(device, lock, door, active(stored.as_deref()).is_some()) {
                Ok(asked) => (asked, Ok(())),
                Err(error) => (false, Err(error)),
            }
        }
        Ok(_) => (false, Ok(())),
        Err(error) => (false, Err(*error)),
    }
}

fn repair(device: &impl Device, stored: &Result<Option<Code>, Error>) -> Result<(), Error> {
    match stored {
        Ok(stored) => device.repair_kind(stored.as_deref()),
        Err(error) => Err(*error),
    }
}

/// What the callback returns: where the screen was asked for, `Ok` (an error
/// would drop the staged command), else the first refusal.
fn settle(asked: bool, rest: Result<(), Error>) -> Result<(), Error> {
    if asked { Ok(()) } else { rest }
}

/// The device woke up: it locks if waking up asks for it (and where that
/// cannot be read, it does), then the kind goes back with the code.
///
/// # Errors
/// The first refusal, once everything that could be done was.
pub fn woke(device: &impl Device, lock: &mut Lockstate, door: &mut Door) -> Result<(), Error> {
    door.opening = false;
    let stored = device.stored();
    let (wake, read) = match device.on_wake() {
        Ok(wake) => (wake, Ok(())),
        Err(error) => (true, Err(error)),
    };
    let (asked, locked) = locked(device, lock, door, &stored, wake);
    let repaired = repair(device, &stored);
    settle(asked, locked.and(read).and(repaired))
}
