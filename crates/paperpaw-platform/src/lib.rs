//! Platform vocabulary for PaperPaw.
//!
//! This crate is the common language between platform shells and the
//! kernel: display descriptions, pixel formats and raw input events. It has
//! no dependencies and never allocates, so it can be used on every target,
//! down to bare-metal microcontrollers.
//!
//! The kernel itself never calls into a platform. It consumes
//! [`InputEvent`]s and emits effects, which a shell executes, for example
//! with a [`Display`] implementation.

#![no_std]

pub mod display;
pub mod geometry;
pub mod input;
pub mod pixel;
pub mod status;
pub mod storage;

pub use display::{ColorPanel, Display, DisplayInfo, FrameView, RefreshMode, Rotation};
pub use geometry::{Point, Rect};
pub use input::{
    ButtonState, InputCaps, InputEvent, Instant, PointerEvent, PointerPhase, PointerTool, buttons,
};
pub use pixel::{Color, PixelFormat};
pub use status::{Battery, SystemStatus, WallClock};
pub use storage::{DirEntry, Mounted, NoStorage, Storage, StorageError};

/// Static description of a device, provided by its shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceInfo {
    /// Stable identifier, e.g. `xteink-x4`.
    pub id: &'static str,
    /// Human-readable model name.
    pub name: &'static str,
    pub display: DisplayInfo,
    pub input: InputCaps,
    pub memory: MemoryClass,
}

impl DeviceInfo {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(
        id: &'static str,
        name: &'static str,
        display: DisplayInfo,
        input: InputCaps,
        memory: MemoryClass,
    ) -> Self {
        Self {
            id,
            name,
            display,
            input,
            memory,
        }
    }

    #[must_use]
    pub const fn with_id(mut self, id: &'static str) -> Self {
        self.id = id;
        self
    }

    #[must_use]
    pub const fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    #[must_use]
    pub const fn with_display(mut self, display: DisplayInfo) -> Self {
        self.display = display;
        self
    }

    #[must_use]
    pub const fn with_input(mut self, input: InputCaps) -> Self {
        self.input = input;
        self
    }

    #[must_use]
    pub const fn with_memory(mut self, memory: MemoryClass) -> Self {
        self.memory = memory;
        self
    }
}

/// How much memory the device has for PaperPaw. Everything must work
/// with [`MemoryClass::Small`]; on large devices plugins may use more to
/// be faster or show more (caches, covers in lists), never to work at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemoryClass {
    /// A microcontroller with a few hundred kilobytes (the Xteink X4).
    Small,
    /// Megabytes and more (Android, Linux readers, desktops).
    Large,
}
