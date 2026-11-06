//! `paperpaw-dev`: checks a PaperPaw plugin before it is published, and
//! PaperPaw's own plugins while they are made.
//!
//! It runs the real kernel with the standard app in the emulator
//! (`shells/desktop`, headless, with its device profiles) and looks at
//! what the plugin adds: every screen and widget drawn on every device, in
//! light and dark, in each language and look, put on contact sheets; what
//! is drawn checked (text cut or overflowing, parts outside their cell,
//! uneven margins, touch targets too small, focus not shown, contrast on
//! black-and-white screens); its texts (missing translations, a
//! pseudo-language that finds text too long, settings without help, ids
//! outside its namespace, dashes); its behaviour (scripted flows, every
//! screen reachable and closable with buttons alone, no storage reads
//! while drawing, time and heap against the X4's budgets); its package
//! (a data plugin as the device installer and the website take it, a
//! `.ppn` file's header and signature); and snapshots to compare with.
//!
//! What it checks:
//!
//! - PaperPaw's own plugins: `cargo run -p paperpaw-dev -- check --plugin
//!   widgets` (the standard app has them all).
//! - A plugin with code, in its own repository: a few lines that add it
//!   to the app (see [`main`]) run as an example or a test.
//! - A data plugin: `paperpaw-dev check path/to/folder`.
//! - A code plugin built for a device: `paperpaw-dev package plugin.ppn`.
//!
//! Everything ends up in a report (text on the console, `report.html`
//! and `report.json` in the output folder, `target/paperpaw-dev` unless
//! `--out` says otherwise); errors make the run fail, for CI.

pub mod cases;
pub mod cli;
pub mod data;
pub mod explore;
pub mod flows;
pub mod gallery;
pub mod image;
pub mod library;
pub mod lint;
pub mod package;
pub mod perf;
pub mod ppn;
pub mod probe;
pub mod publish;
pub mod reach;
pub mod report;
pub mod session;
pub mod snapshot;
pub mod target;
pub mod ui_kit;
pub mod variant;
pub mod vary;
pub mod visual;

use std::process::ExitCode;

pub use paperpaw_emulator::device::AddPlugins;

/// Runs `paperpaw-dev` with the command line of this process, the app
/// having the plugins `add` adds: the entry point for a plugin's own
/// repository, e.g. `examples/paperpaw-dev.rs`:
///
/// ```no_run
/// fn main() -> std::process::ExitCode {
///     paperpaw_dev::main(|app| app /* .plugin(my_plugin::MyPlugin) */)
/// }
/// ```
///
/// and then `cargo run --example paperpaw-dev -- check --plugin my-plugin`.
pub fn main(add: AddPlugins) -> ExitCode {
    cli::main(std::env::args().skip(1), Some(add))
}
