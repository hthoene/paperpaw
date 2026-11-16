//! PaperPaw in a browser: the standard app on an emulated X4, X4 Pro or
//! Boox, drawn into a canvas, with data plugins handed in from the page
//! and `paperpaw-dev`'s checks of them. The desktop emulator's device
//! model (`paperpaw-emulator` without its window) built for
//! `wasm32-unknown-unknown`: the same kernel, profiles, panel and heap
//! budgets as `paperpaw-emu` and `paperpaw-dev`.
//!
//! - [`emulator`]: the device and what the page asks of it, testable
//!   without a browser.
//! - `exports` (in the WebAssembly module only): the functions the page
//!   calls.
//! - `www/`: the page's side: `paperpaw-web.js` loads the module and wraps
//!   it, `frame.html` speaks the emulator frame's message protocol (see
//!   `docs/web-emulator.md`), `index.html` is a page to try it.

pub mod emulator;
#[cfg(target_arch = "wasm32")]
pub mod exports;
pub mod network;
