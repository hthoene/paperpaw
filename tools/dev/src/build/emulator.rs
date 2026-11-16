//! `build --device emulator`: the plugin's program as the library the
//! emulator loads on this computer (`code-host.so`; docs/plugins.md, "The
//! file in the emulator").
//!
//! The emulator takes a library that imports nothing and exports its two
//! entry symbols and its heap word alone. The plugin's crate is a program for the devices, so
//! this links the same program as such a library: without the C startup
//! files, with the memory functions the compiler calls, with the entry
//! record among the relocated read-only data, and with everything but
//! those three symbols kept local. `tools/plugin-host-libs` builds PaperPaw's
//! own plugins so; its two files are used here.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{FOLDER, PAPERPAW, REPOSITORY, copy, step};

const TRIPLE: &str = "x86_64-unknown-linux-gnu";
/// The file the emulator loads a plugin's code from, in the plugin's folder.
const FILE: &str = "code-host.so";
/// `memcpy` and the like, which such a library brings itself.
const MEMORY: &str = include_str!("../../../plugin-host-libs/mem.s");
const LINKER: &str = include_str!("../../../plugin-host-libs/paperpaw-host.ld");
const EXPORTS: &str = "{ global: PAPERPAW_ENTRY; PAPERPAW_METADATA; PAPERPAW_HEAP; local: *; };\n";

/// Writes `text` to `name` in `work`.
fn kept(work: &Path, name: &str, text: &str) -> Result<PathBuf, String> {
    let path = work.join(name);
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Builds the plugin `id` of `dir` for the emulator on this computer and
/// leaves `dist/emulator`, a library with the plugin in it.
pub(super) fn build(dir: &Path, id: &str, target: &Path) -> Result<bool, String> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(
            "the emulator runs a plugin's code on Linux (x86-64) only: build it there, or \
             for a device"
                .into(),
        );
    }
    // The linker is told where these are: whole paths.
    let work = target.join("paperpaw-dev-emulator");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let work = work
        .canonicalize()
        .map_err(|e| format!("{}: {e}", work.display()))?;
    let memory = kept(&work, "mem.s", &format!(".intel_syntax noprefix\n{MEMORY}"))?;
    let linker = kept(&work, "paperpaw-host.ld", LINKER)?;
    let exports = kept(&work, "exports.map", EXPORTS)?;
    let object = work.join("mem.o");
    step(
        Command::new("cc")
            .args(["-c", "-x", "assembler"])
            .arg(&memory)
            .arg("-o")
            .arg(&object),
        "the C compiler `cc` (Rust links with it; e.g. `sudo apt install gcc`)",
    )?;
    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(dir)
        .args(["rustc", "--release", "--target", TRIPLE, "--bin", id, "--"]);
    for arg in [
        "-shared".to_string(),
        "-nostartfiles".to_string(),
        format!("-Wl,-T{}", linker.display()),
        format!("-Wl,--version-script={}", exports.display()),
        object.display().to_string(),
    ] {
        cargo.arg("-C").arg(format!("link-arg={arg}"));
    }
    step(&mut cargo, "the build (cargo)")?;

    let library = dir.join("dist").join("emulator");
    let folder = library
        .join(paperpaw_core::sdk::builtin::PLUGIN_DIR)
        .join(id);
    let _ = std::fs::remove_dir_all(&folder);
    copy(&dir.join(FOLDER), &folder)?;
    let _ = std::fs::remove_dir_all(folder.join("store"));
    let built = target.join(TRIPLE).join("release").join(id);
    let file = folder.join(FILE);
    std::fs::copy(&built, &file).map_err(|e| format!("{}: {e}", built.display()))?;
    println!(
        "\n{library} is a library for the emulator with the plugin in it ({file}). Start it \
         as an X4, with the X4's memory:\n    \
         paperpaw-emu --library {library} --trust-code\n\
         What the emulator keeps (settings, the plugin's files) stays in that folder; build \
         again and start again to see a change.\n\
         The emulator comes with PaperPaw and takes the plugin only if the same compiler \
         built both. To install it, in the plugin's folder (for its compiler):\n    \
         cargo install --locked --git {REPOSITORY} --tag v{PAPERPAW} paperpaw-emulator\n\
         or, from a PaperPaw checkout:\n    \
         cargo install --locked --path <checkout>/shells/desktop",
        library = library.display(),
        file = file.display(),
    );
    Ok(true)
}
