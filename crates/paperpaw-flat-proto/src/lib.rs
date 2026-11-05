//! Prototype for #228 (do not merge): what a flat `extern "C"` plugin
//! boundary costs on the X4, next to the Rust-shaped one used now.
//!
//! - [`rusty`]: the boundary as it is today for drawing and text: a table of
//!   function pointers with Rust's own calling convention and Rust types
//!   (`paperpaw_table::shared!` entries), and a plugin's handler as a boxed
//!   closure that pushes `Command`s.
//! - [`flat`]: the same calls through a `#[repr(C)]` table of `extern "C"`
//!   functions with plain types (integers, pointers and lengths only), the
//!   host side that adapts them to the real `Canvas` and text functions, and
//!   a plugin's `deliver` entry that takes bytes and writes bytes.
//! - [`widget`]: the drawing core of the real "Battery" widget (its shell,
//!   knob, level, bolt and number) written once against a small `Gfx` trait,
//!   used with either table, to measure a plugin ported to the flat table.
//! - [`bench`]: the same work through both paths, timed.

#![no_std]
#![allow(
    unsafe_code,
    reason = "a prototype of a C boundary: raw pointers and extern fns are the point"
)]

extern crate alloc;

pub mod bench;
pub mod flat;
pub mod rusty;
pub mod widget;
