//! Native code plugins as files (ADR 0024), as the plugin, the firmware
//! and the tools that make them agree on: what a plugin file (`.ppn`)
//! holds and how it is checked ([`Header`]), how its image is moved to
//! where it runs ([`Relocator`]), the table the firmware gives every
//! plugin ([`Api`]) and the plugin's entry ([`Entry`]), and, for the X4,
//! the index of plugins kept in flash ([`index`]) behind the firmware's
//! image ([`ImageExtent`]), where the web flasher also leaves a
//! [`bundle`] of plugins to install. Authors sign their files
//! ([`signature`]).
//! Plugins get their entry from the SDK's `plugin!`; `paperpaw-sign pack`
//! makes the files.
//!
//! # The file
//!
//! All numbers little-endian.
//!
//! | Bytes | What |
//! |-------|------|
//! | 0..4 | `PPNP` |
//! | 4..6 | the format, 3 |
//! | 6..8 | the chip ([`Chip`]: 5 the ESP32-C3 of the X4, 9 the ESP32-S3 of the X4 Pro) |
//! | 8..16 | the fingerprint: the plugin interface, the chip and the layouts it was built for (the SDK's `native::FINGERPRINT`) |
//! | 16..20 | `base`: the data address the image is linked at |
//! | 20..24 | `image_len`: the image's bytes |
//! | 24..28 | `code_at`: where the code begins in the image |
//! | 28..32 | `entry`: where the [`Entry`] lies in the image |
//! | 32..36 | `api`: the word in the image the loader writes the address of the firmware's [`Api`] into |
//! | 36..40 | `relocs`: how many words are to relocate |
//! | 40..44 | `align`: what the image's address must be a multiple of (4 to 4096) |
//! | 44..48 | the compiler it was built with: [`compiler::id`] of its release, never 0 |
//! | 48..52 | `hot_at`: where the hot code begins in the image, in the code (0 if there is none) |
//! | 52..56 | `hot_len`: the hot code's bytes, a multiple of 4 (0: none) |
//! | 56..60 | `patches`: how many patches follow the words to relocate |
//! | 60..92 | SHA-256 of bytes 0..60 and of everything after the header |
//! | 92.. | the image: read-only data, then code from `code_at` |
//! | then | `relocs` offsets into the image (u32 each, increasing) of the 32-bit words that hold an address in the plugin |
//! | then | `patches` records of two u32, in increasing order of the first: `site \| kind << 28` and a reference ([`PatchKind`]) |
//! | then | the author's signature, if signed ([`signature`], 112 bytes) |
//!
//! # Where an image runs
//!
//! On both chips the MMU serves the data bus and the instruction bus
//! alike: memory seen at data address A runs at A + [`CODE_OFFSET`]. A
//! plugin's read-only data and code are linked as one block on that
//! basis (`paperpaw-plugin.x`), so code reaches its data and itself by
//! relative addresses, which stay right wherever the block lands. Only
//! the words that hold an address (vtables, function pointers, Xtensa's
//! literals) change: the loader adds the distance moved to each, and
//! writes the address of the firmware's table into the [`Api`] word.
//!
//! - The X4 Pro reads the image into PSRAM and runs it there.
//! - The X4 writes it into flash once, behind its firmware, and maps it;
//!   the [`index`] says what lies there, so later starts only map.
//!
//! A plugin has no writable data: a static it could change would need RAM
//! at an address fixed in its code. Its state lives on the kernel's heap.
//!
//! # Hot code
//!
//! A plugin may mark code as hot (the SDK's `hot!`): small loops that run
//! all the time. The linker script keeps it at the end of the code, and
//! the file says where (`hot_at`, `hot_len`). A device with faster memory
//! than its code's may run it from there: the X4 copies it into internal
//! RAM, as the [`index`] says where. The image then no longer moves as
//! one block, so what reaches across the hot code's edge by a distance
//! changes: the file lists those instructions as patches, which the
//! [`Relocator`] writes anew. Only RISC-V moves hot code (`auipc` with
//! `jalr`, `addi`, a load or a store); an Xtensa file has none, its hot
//! code stays in place.

#![no_std]

extern crate alloc;

pub mod bundle;
pub mod compiler;
mod extent;
mod header;
pub mod index;
mod interface;
mod reloc;
pub mod shared;
pub mod signature;

pub use extent::ImageExtent;
pub use header::{
    CODE_OFFSET, Chip, FORMAT, HEADER_LEN, Header, LINK_BASE, MAGIC, MAX_IMAGE_LEN, Problem,
};
pub use interface::{API_SYMBOL, Api, ENTRY_SECTION, ENTRY_SYMBOL, Entry};
pub use reloc::{PatchKind, Relocator, check_relocations};
pub use signature::Signature;
