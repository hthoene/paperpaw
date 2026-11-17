//! What the host found of the fonts and the settings, kept beside them: the
//! types plugins and PaperPaw share hold no memory of it, so they stay the
//! same for every build of one interface.
use alloc::rc::Rc;
use core::ops::{Deref, DerefMut};
use paperpaw_sdk::{RoleMemo, SettingMemo};

use super::font_removals::FontRemovals;

/// The font of each built-in role and the place of the settings looked up
/// last, as [`Watched`] forgets them.
pub(crate) struct HostMemos {
    pub(crate) roles: RoleMemo,
    pub(crate) places: SettingMemo,
}

impl core::fmt::Debug for HostMemos {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("HostMemos")
    }
}

impl HostMemos {
    pub(crate) const fn new() -> Self {
        Self {
            roles: RoleMemo::new(),
            places: SettingMemo::new(),
        }
    }
}

/// A value of the kernel whose every change (each mutable borrow of it)
/// makes the host forget what it found of it: so that nothing found is ever
/// stale, wherever the value is changed.
pub(crate) struct Watched<T> {
    value: T,
    memos: Rc<FontRemovals>,
    forget: fn(&HostMemos),
}

impl<T> Watched<T> {
    pub(crate) fn new(value: T, memos: Rc<FontRemovals>, forget: fn(&HostMemos)) -> Self {
        Self {
            value,
            memos,
            forget,
        }
    }
}

/// [`Watched`] fonts: the roles' memo is forgotten.
pub(crate) fn forget_roles(memos: &HostMemos) {
    memos.roles.forget();
}

/// [`Watched`] settings: the memo of places is forgotten.
pub(crate) fn forget_places(memos: &HostMemos) {
    memos.places.forget();
}

impl<T> Deref for Watched<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Watched<T> {
    fn deref_mut(&mut self) -> &mut T {
        (self.forget)(&self.memos.memos);
        &mut self.value
    }
}
