//! The firmware's shared function tables (ADR 0026, #44).
//!
//! A function that every plugin would carry a copy of, and the firmware
//! has anyway, can be *shared*: the crate that has it declares it with
//! [`shared!`] and an index of [`index`]. In the firmware, on computers
//! and phones, and on the X4 Pro's chip it is the function it always was.
//! In a plugin built for a RISC-V chip (`--cfg paperpaw_plugin`) its body
//! is left out and it calls the firmware's copy instead, through its
//! [`Family`]'s table, with Rust's own calling convention, which the
//! plugin interface pins (same compiler, same sources, same layouts).
//!
//! Functions come in families (text, fonts, ...), each with a table of
//! its own and a revision ([`Revisions`]). `paperpaw-plugin.x` makes one
//! word per family, `PAPERPAW_TABLE_0` to `PAPERPAW_TABLE_7`, which the
//! loader fills with the address of the firmware's table (null where it
//! has none); the packer sees which of them a plugin's code reads, so a
//! plugin file says which families it uses. A plugin runs where [`fits`]
//! says so for each of them.
//!
//! `shared.list` is the interface: entries are only ever appended, each
//! at a new revision of its family. `tools/shared-table.py` writes
//! [`index`] and `table.sig` (the signature of every entry) from it.

#![no_std]

/// A family of shared functions: one table, one revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Family(u8);

impl Family {
    /// Measuring and drawing text: `paperpaw_text`, the SDK's font
    /// handles.
    pub const TEXT: Family = Family(0);
    /// The SDK's fonts beyond the handles.
    pub const FONTS: Family = Family(1);
    /// The SDK's interface blocks (`ui`).
    pub const UI: Family = Family(2);
    /// Drawing (`paperpaw_graphics`).
    pub const GRAPHICS: Family = Family(3);
    /// Registering what a plugin contributes.
    pub const CONTRIBUTION: Family = Family(4);

    /// The family of number `n` (below [`FAMILIES`]).
    #[must_use]
    pub const fn of(n: u8) -> Option<Family> {
        if (n as usize) < FAMILIES {
            Some(Family(n))
        } else {
            None
        }
    }

    /// Its number: where its revision is kept in [`Revisions`], and the
    /// `n` of its word `PAPERPAW_TABLE_<n>`.
    #[must_use]
    pub const fn number(self) -> usize {
        self.0 as usize
    }
}

/// How many families there can be: the words of `paperpaw-plugin.x`.
pub const FAMILIES: usize = 8;

/// A revision per family, by [`Family::number`]; 0: none (a firmware
/// without the family, a plugin that does not use it).
pub type Revisions = [u16; FAMILIES];

/// Whether a plugin built with the revisions `plugin` runs on a firmware
/// whose families are at `current`, and which runs plugins built from
/// `oldest` on: for every family the plugin uses (its revision is not 0),
/// `oldest <= plugin <= current`. A family the firmware lacks is at 0.
#[must_use]
pub const fn fits(plugin: &Revisions, oldest: &Revisions, current: &Revisions) -> bool {
    let mut i = 0;
    while i < FAMILIES {
        let wanted = plugin[i];
        if wanted != 0 && (wanted < oldest[i] || wanted > current[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// The addresses of one family's shared functions, in the order of its
/// [`index`]. The firmware builds it; a table is a plain array, never
/// written.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Table(pub *const *const ());

impl Table {
    /// No table: a family the firmware does not share.
    pub const NONE: Table = Table(core::ptr::null());
}

// SAFETY: addresses of functions that live as long as the firmware runs,
// never written.
unsafe impl Sync for Table {}
// SAFETY: as above.
unsafe impl Send for Table {}

/// The index of each shared function in its family's table, and the
/// revisions of this build ([`REVISIONS`], [`OLDEST`]).
#[rustfmt::skip]
pub mod index {
    include!("index.rs");
}

/// The revision of each family this build of the SDK has: what a plugin
/// built with it needs ([`fits`]).
pub const REVISIONS: Revisions = index::REVISIONS;
/// The oldest revision of each family whose plugins still run with the
/// bodies of this build: what a firmware built with it accepts.
pub const OLDEST: Revisions = index::OLDEST;

/// The firmware's function at `index` of `family`'s table, as the
/// function pointer `F`.
///
/// # Safety
///
/// `F` must be the type of the function the table keeps at `index`, and
/// the loader must have checked that the firmware's revision of `family`
/// is at least this build's ([`fits`]).
#[cfg(all(paperpaw_plugin, target_os = "none", target_arch = "riscv32"))]
#[inline(always)]
pub unsafe fn slot<F: Copy>(family: Family, index: usize) -> F {
    const { assert!(core::mem::size_of::<F>() == core::mem::size_of::<usize>()) };
    unsafe extern "C" {
        static PAPERPAW_TABLE_0: Table;
        static PAPERPAW_TABLE_1: Table;
        static PAPERPAW_TABLE_2: Table;
        static PAPERPAW_TABLE_3: Table;
        static PAPERPAW_TABLE_4: Table;
        static PAPERPAW_TABLE_5: Table;
        static PAPERPAW_TABLE_6: Table;
        static PAPERPAW_TABLE_7: Table;
    }
    // SAFETY: words `paperpaw-plugin.x` makes and the loader writes
    // before any code of the plugin runs, to tables that live as long as
    // the firmware runs; the caller's contract for `F` and the index.
    unsafe {
        let table = match family.0 {
            0 => PAPERPAW_TABLE_0,
            1 => PAPERPAW_TABLE_1,
            2 => PAPERPAW_TABLE_2,
            3 => PAPERPAW_TABLE_3,
            4 => PAPERPAW_TABLE_4,
            5 => PAPERPAW_TABLE_5,
            6 => PAPERPAW_TABLE_6,
            _ => PAPERPAW_TABLE_7,
        };
        core::mem::transmute_copy::<*const (), F>(&*table.0.add(index))
    }
}

/// Declares a function shared through its family's table: it is the
/// function written, except in a plugin built for a RISC-V chip, where it
/// calls the firmware's copy. Its parameters are plain `name: Type` (and
/// `&self`, `&mut self`); a function with generics other than lifetimes,
/// or `impl Trait` parameters, keeps a generic shell in the plugin that
/// calls a shared non-generic one. `@ family::INDEX` names its place in
/// [`index`].
///
/// ```ignore
/// paperpaw_table::shared! {
///     /// Width of `text`.
///     pub fn measure(font: &Font, size: f32, text: &str) -> f32 {
///         /* the body */
///     } @ text::MEASURE
/// }
/// ```
#[macro_export]
macro_rules! shared {
    // A function.
    ($(#[$meta:meta])* $vis:vis fn $name:ident $(<$($lt:lifetime),+>)?
     ($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block
     @ $family:ident :: $index:ident) => {
        $(#[$meta])*
        $vis fn $name $(<$($lt),+>)? ($($arg: $ty),*) $(-> $ret)? {
            #[cfg(all(paperpaw_plugin, target_os = "none", target_arch = "riscv32"))]
            {
                // SAFETY: the firmware's table holds this very function,
                // of this type, at this index (`tools/shared-table.py`),
                // and the loader checked the family's revision.
                let call: fn($($ty),*) $(-> $ret)? = unsafe {
                    $crate::slot(
                        $crate::index::$family::FAMILY,
                        $crate::index::$family::$index,
                    )
                };
                call($($arg),*)
            }
            #[cfg(not(all(paperpaw_plugin, target_os = "none", target_arch = "riscv32")))]
            $body
        }
    };
    // A method taking `&self`.
    ($(#[$meta:meta])* $vis:vis fn $name:ident $(<$($lt:lifetime),+>)?
     (&$this:ident $(, $arg:ident : $ty:ty)* $(,)?) $(-> $ret:ty)? $body:block
     @ $family:ident :: $index:ident) => {
        $(#[$meta])*
        $vis fn $name $(<$($lt),+>)? (&$this $(, $arg: $ty)*) $(-> $ret)? {
            #[cfg(all(paperpaw_plugin, target_os = "none", target_arch = "riscv32"))]
            {
                // SAFETY: as above.
                let call: fn(&Self $(, $ty)*) $(-> $ret)? = unsafe {
                    $crate::slot(
                        $crate::index::$family::FAMILY,
                        $crate::index::$family::$index,
                    )
                };
                call($this $(, $arg)*)
            }
            #[cfg(not(all(paperpaw_plugin, target_os = "none", target_arch = "riscv32")))]
            $body
        }
    };
    // A method taking `&mut self`.
    ($(#[$meta:meta])* $vis:vis fn $name:ident $(<$($lt:lifetime),+>)?
     (&mut $this:ident $(, $arg:ident : $ty:ty)* $(,)?) $(-> $ret:ty)? $body:block
     @ $family:ident :: $index:ident) => {
        $(#[$meta])*
        $vis fn $name $(<$($lt),+>)? (&mut $this $(, $arg: $ty)*) $(-> $ret)? {
            #[cfg(all(paperpaw_plugin, target_os = "none", target_arch = "riscv32"))]
            {
                // SAFETY: as above.
                let call: fn(&mut Self $(, $ty)*) $(-> $ret)? = unsafe {
                    $crate::slot(
                        $crate::index::$family::FAMILY,
                        $crate::index::$family::$index,
                    )
                };
                call($this $(, $arg)*)
            }
            #[cfg(not(all(paperpaw_plugin, target_os = "none", target_arch = "riscv32")))]
            $body
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_fits_between_the_oldest_and_the_current_revision() {
        let current = [3, 1, 0, 0, 0, 0, 0, 0];
        let oldest = [2, 1, 0, 0, 0, 0, 0, 0];
        assert!(fits(&[0; FAMILIES], &oldest, &current));
        assert!(fits(&[2, 0, 0, 0, 0, 0, 0, 0], &oldest, &current));
        assert!(fits(&[3, 1, 0, 0, 0, 0, 0, 0], &oldest, &current));
        // Built before a change of the text family's bodies.
        assert!(!fits(&[1, 0, 0, 0, 0, 0, 0, 0], &oldest, &current));
        // Built with a newer text family, or a family this firmware lacks.
        assert!(!fits(&[4, 0, 0, 0, 0, 0, 0, 0], &oldest, &current));
        assert!(!fits(&[0, 0, 1, 0, 0, 0, 0, 0], &oldest, &current));
    }

    #[test]
    fn families_are_numbered_below_the_words() {
        assert_eq!(Family::of(7).map(Family::number), Some(7));
        assert_eq!(Family::of(8), None);
        assert_eq!(Family::CONTRIBUTION.number(), 4);
    }
}
