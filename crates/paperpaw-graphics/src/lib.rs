//! Framebuffers and drawing primitives for PaperPaw.
//!
//! A [`Framebuffer`] stores pixels in the device's native [`PixelFormat`], so
//! a 800×480 1-bit screen needs 48 KB instead of 384 KB. Drawing goes through
//! a [`Canvas`], which clips to a region and records which part of the screen
//! changed, so the kernel can refresh only that part of the e-ink panel.

#![no_std]

extern crate alloc;

mod band;
mod blit;
mod canvas;
pub mod diffuse;
mod framebuffer;
mod mask;
mod probe;
pub mod round;

pub use band::{BandView, FramePass, Region, RegionKind};
pub use blit::{Bitmap, BitmapError, BitmapFormat, Blit, Dither};
pub use canvas::{Canvas, Describe, Described, Role};
pub use framebuffer::{Framebuffer, MONO_COVERAGE};
pub use probe::InkBounds;

use core::fmt;
use paperpaw_platform::PixelFormat;

/// Errors of this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphicsError {
    /// Not enough memory for the requested buffer.
    OutOfMemory { bytes: usize },
    /// The pixel format is not compiled in (see the `color` feature).
    UnsupportedFormat(PixelFormat),
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl fmt::Display for GraphicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphicsError::OutOfMemory { bytes } => {
                write!(f, "out of memory allocating {bytes} bytes")
            }
            GraphicsError::UnsupportedFormat(format) => {
                write!(f, "pixel format {format:?} is not supported by this build")
            }
            GraphicsError::Other(code) => write!(f, "graphics error {code}"),
        }
    }
}

impl core::error::Error for GraphicsError {}
