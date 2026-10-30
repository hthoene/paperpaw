//! The firmware's shared function table (prototype of option B2, #44).
//!
//! A function that every plugin would carry a copy of, and the firmware
//! has anyway, can be *shared*: the crate that has it declares it with
//! [`shared!`](crate::shared!) and an [`index`]. In the firmware (and on a
//! computer) it is the function it always was. In a plugin built for a
//! chip with `--cfg paperpaw_plugin` its body is left out and it calls the
//! firmware's address at that index of [`Api::shared`](crate::Api::shared)
//! instead, with Rust's own calling convention, which the plugin interface
//! pins (same compiler, same crate sources, same layouts, ADR 0026).
//!
//! The order of [`index`] is the plugin interface: entries are only ever
//! added at its end (and a plugin says how many it was built for, see
//! [`Entry::needs`](crate::Entry::needs)). The file is made by
//! `tools/shared-table.py` from `shared.list`.



/// The addresses of the shared functions, in the order of [`index`]. The
/// firmware builds it (`paperpaw_sdk::shared::TABLE`); a table is a
/// plain array, never written.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Table(pub *const *const ());

impl Table {
    /// No table: a shell that lets plugins keep their own copies.
    pub const NONE: Table = Table(core::ptr::null());
}

// SAFETY: addresses of functions that live as long as the firmware runs,
// never written.
unsafe impl Sync for Table {}
// SAFETY: as above.
unsafe impl Send for Table {}

/// The index of each shared function in the table, and how many there
/// are.
pub mod index {
    include!("shared_index.rs");
}

/// How many entries a plugin built with this crate needs of the table.
pub const NEEDS: u32 = index::COUNT as u32;

/// The firmware's address at `index`, as the function pointer `F`.
///
/// # Safety
///
/// `F` must be the type of the function the table keeps at `index`, and
/// the firmware's table must be at least [`NEEDS`] long (the loader has
/// seen to it).
#[cfg(all(paperpaw_plugin, target_os = "none"))]
#[inline(always)]
pub unsafe fn slot<F: Copy>(index: usize) -> F {
    const { assert!(core::mem::size_of::<F>() == core::mem::size_of::<usize>()) };
    unsafe extern "C" {
        /// The firmware's table: `paperpaw-plugin.x` makes the word, the
        /// loader writes its address before the plugin's code runs.
        static PAPERPAW_API: *const crate::Api;
    }
    // SAFETY: a table that lives as long as the firmware runs, written
    // before any code of the plugin ran; the caller's contract for `F`
    // and the index.
    unsafe {
        let table = (*PAPERPAW_API).shared.0;
        core::mem::transmute_copy::<*const (), F>(&*table.add(index))
    }
}

/// Declares a function shared through the table: it is the function
/// written, except in a plugin built for a chip, where it calls the
/// firmware's copy at `$index`. Its parameters are plain `name: Type`
/// (and `&self` or `&mut self`); a function with generics other than
/// lifetimes, or `impl Trait` parameters, keeps a generic shell in the
/// plugin that calls a shared non-generic one.
///
/// ```ignore
/// paperpaw_native::shared! {
///     /// Width of `text`.
///     pub fn measure(font: &Font, size: f32, text: &str) -> f32 {
///         /* the body */
///     } @ MEASURE
/// }
/// ```
#[macro_export]
macro_rules! shared {
    // A function.
    ($(#[$meta:meta])* $vis:vis fn $name:ident $(<$($lt:lifetime),+>)?
     ($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block @ $index:ident) => {
        $(#[$meta])*
        $vis fn $name $(<$($lt),+>)? ($($arg: $ty),*) $(-> $ret)? {
            #[cfg(all(paperpaw_plugin, target_os = "none"))]
            {
                // SAFETY: the firmware's table holds this very function,
                // of this type, at this index (`tools/shared-table.py`).
                let call: fn($($ty),*) $(-> $ret)? =
                    unsafe { $crate::shared::slot($crate::shared::index::$index) };
                call($($arg),*)
            }
            #[cfg(not(all(paperpaw_plugin, target_os = "none")))]
            $body
        }
    };
    // A method taking `&self`.
    ($(#[$meta:meta])* $vis:vis fn $name:ident $(<$($lt:lifetime),+>)?
     (&$this:ident $(, $arg:ident : $ty:ty)* $(,)?) $(-> $ret:ty)? $body:block @ $index:ident) => {
        $(#[$meta])*
        $vis fn $name $(<$($lt),+>)? (&$this $(, $arg: $ty)*) $(-> $ret)? {
            #[cfg(all(paperpaw_plugin, target_os = "none"))]
            {
                // SAFETY: as above.
                let call: fn(&Self $(, $ty)*) $(-> $ret)? =
                    unsafe { $crate::shared::slot($crate::shared::index::$index) };
                call($this $(, $arg)*)
            }
            #[cfg(not(all(paperpaw_plugin, target_os = "none")))]
            $body
        }
    };
    // A method taking `&'a mut self`, `'a` named in its other parameters.
    ($(#[$meta:meta])* $vis:vis fn $name:ident <$($lt:lifetime),+>
     (&$self_lt:lifetime mut $this:ident $(, $arg:ident : $ty:ty)* $(,)?) $(-> $ret:ty)? $body:block @ $index:ident) => {
        $(#[$meta])*
        $vis fn $name <$($lt),+> (&$self_lt mut $this $(, $arg: $ty)*) $(-> $ret)? {
            #[cfg(all(paperpaw_plugin, target_os = "none"))]
            {
                // SAFETY: as above.
                let call: fn(&$self_lt mut Self $(, $ty)*) $(-> $ret)? =
                    unsafe { $crate::shared::slot($crate::shared::index::$index) };
                call($this $(, $arg)*)
            }
            #[cfg(not(all(paperpaw_plugin, target_os = "none")))]
            $body
        }
    };
    // A method taking `&mut self`.
    ($(#[$meta:meta])* $vis:vis fn $name:ident $(<$($lt:lifetime),+>)?
     (&mut $this:ident $(, $arg:ident : $ty:ty)* $(,)?) $(-> $ret:ty)? $body:block @ $index:ident) => {
        $(#[$meta])*
        $vis fn $name $(<$($lt),+>)? (&mut $this $(, $arg: $ty)*) $(-> $ret)? {
            #[cfg(all(paperpaw_plugin, target_os = "none"))]
            {
                // SAFETY: as above.
                let call: fn(&mut Self $(, $ty)*) $(-> $ret)? =
                    unsafe { $crate::shared::slot($crate::shared::index::$index) };
                call($this $(, $arg)*)
            }
            #[cfg(not(all(paperpaw_plugin, target_os = "none")))]
            $body
        }
    };
}
