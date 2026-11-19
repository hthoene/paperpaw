//! PaperPaw's volume driver: FAT32 today, exFAT behind the same interface next.
//!
//! The FAT code is rust-fatfs 0.4 (MIT, Rafał Harabień), carried here with
//! PaperPaw's changes and cut down: no `log`, no `chrono`, FAT12/16 and
//! formatting only behind features for tests. Every file system in this crate
//! is mounted through [`detect`], which reads the boot sector and says which
//! driver owns the volume, so a second type plugs in beside [`FileSystem`].

#![cfg_attr(not(test), no_std)]
// Carried code, not written to the workspace's lint rules (it keeps rust-fatfs's
// style); its behaviour is held by the tests of this crate and of paperpaw-fat.
#![allow(
    clippy::all,
    unused_macros,
    dead_code,
    unused_imports,
    elided_lifetimes_in_paths,
    missing_debug_implementations
)]

extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

#[macro_use]
mod log_macros;

mod boot_sector;
mod dir;
mod dir_entry;
mod error;
mod exfat;
mod file;
#[cfg(any(feature = "format", test))]
mod format;
mod fs;
mod io;
mod kind;
mod names;
mod table;
mod time;
pub mod vol;

pub use crate::dir::*;
pub use crate::dir_entry::*;
pub use crate::error::*;
#[cfg(any(feature = "format", test))]
pub use crate::exfat::format::format_exfat;
pub use crate::file::*;
#[cfg(any(feature = "format", test))]
pub use crate::format::*;
pub use crate::fs::*;
pub use crate::io::*;
pub use crate::kind::*;

pub use crate::time::*;
