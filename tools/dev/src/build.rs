//! `build`: a plugin with code, from its own repository to the file a device
//! loads.
//!
//! In PaperPaw's repository `tools/plugin-files7/build.sh` does this for the
//! plugins of PaperPaw's own. An author's repository (what `new --code`
//! writes) has the crate, the linker script and the build configuration, and
//! this command does the rest in order: the records of what the code calls
//! ([`crate::metadata`]), a look at the compiler before Cargo fails in its
//! own words, the build for the chip asked for, and the packing, which
//! `paperpaw-sign` does (a program of its own that comes with PaperPaw).
//!
//! `build [<plugin's folder>] [--device <x4|x3|x4-pro|x4-classic>]
//! [--sign-tool <path>]` leaves `dist/<device>/<id>/`: the plugin's folder as
//! it goes onto a card, with the packed code in it. The repository's own
//! `plugin` folder is not touched (the plugin store takes no code file in
//! it).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The PaperPaw release this tool is of.
const PAPERPAW: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
/// The file a device loads a plugin's code from, in the plugin's folder.
const FILE: &str = "code.ppn";
/// The plugin's folder in its repository: its `plugin.ini`, words, pictures.
const FOLDER: &str = "plugin";

/// What a device's chip needs of the build.
struct Chip {
    /// As `--device` names it.
    device: &'static str,
    /// The devices that take this build.
    devices: &'static str,
    triple: &'static str,
    /// The toolchain Cargo is asked for by name; the X4's is the one
    /// `rust-toolchain.toml` names.
    toolchain: Option<&'static str>,
}

const X4: Chip = Chip {
    device: "x4",
    devices: "the Xteink X4 and X3",
    triple: "riscv32imc-unknown-none-elf",
    toolchain: None,
};
const X4_PRO: Chip = Chip {
    device: "x4-pro",
    devices: "the Xteink X4 Pro and X4 Classic",
    triple: "xtensa-esp32s3-none-elf",
    toolchain: Some("esp"),
};

/// What `program args..` prints, `None` if it cannot be started or fails.
fn said(dir: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env_remove("RUSTFLAGS")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// What is missing of the compiler for `chip`, with how to get it; and for
/// the X4 Pro whether the core libraries must be built with the plugin.
fn compiler(dir: &Path, chip: &Chip) -> Result<bool, String> {
    let Some(toolchains) = said(dir, "rustup", &["toolchain", "list"]) else {
        return Err(
            "rustup was not found: install Rust from https://rustup.rs, then run \
                    this again"
                .into(),
        );
    };
    let Some(toolchain) = chip.toolchain else {
        // rustup installs what `rust-toolchain.toml` names when it is first
        // asked; a target left out of that file is the one thing to add.
        let targets = said(dir, "rustup", &["target", "list", "--installed"]).unwrap_or_default();
        if !targets.lines().any(|target| target.trim() == chip.triple) {
            return Err(format!(
                "the compiler for {} is not installed. In the plugin's folder run:\n    \
                 rustup target add {}",
                chip.devices, chip.triple
            ));
        }
        return Ok(false);
    };
    if !toolchains
        .lines()
        .any(|line| line.split_whitespace().next() == Some(toolchain))
    {
        return Err(format!(
            "the compiler for {} is not installed (rustup's toolchain `{toolchain}`). Run:\n    \
             cargo install espup --locked\n    espup install --targets esp32s3\n\
             and, in each new terminal before building, what espup says at its end \
             (`. ~/export-esp.sh` on Linux and macOS).",
            chip.devices
        ));
    }
    // The core libraries: a PaperPaw release installs them into the
    // toolchain (`tools/xtensa-std`); without them they are built along.
    let sysroot = said(
        dir,
        "rustc",
        &[&format!("+{toolchain}"), "--print", "sysroot"],
    );
    let installed = sysroot.is_some_and(|sysroot| {
        let libraries = Path::new(sysroot.trim())
            .join("lib/rustlib")
            .join(chip.triple)
            .join("lib");
        std::fs::read_dir(libraries).is_ok_and(|mut files| {
            files.any(|file| {
                file.is_ok_and(|f| f.file_name().to_string_lossy().starts_with("libcore-"))
            })
        })
    });
    Ok(!installed)
}

/// The crate's name, which is the plugin's id (`new --code` writes it so).
fn package(dir: &Path) -> Result<String, String> {
    let manifest = dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| {
        format!(
            "{}: {e}. `build` is run in the folder of a plugin with code \
             (`paperpaw-dev new --github <id> --code` makes one)",
            manifest.display()
        )
    })?;
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some(value) = line.strip_prefix("name")
            && let Some(value) = value.trim_start().strip_prefix('=')
        {
            return Ok(value.trim().trim_matches('"').to_string());
        }
    }
    Err(format!("{}: no name under [package]", manifest.display()))
}

/// The packer: the one named, `$PAPERPAW_SIGN`, the one beside this
/// program, or the one on the path.
fn sign_tool(named: Option<PathBuf>) -> Result<PathBuf, String> {
    let name = format!("paperpaw-sign{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|path| path.is_file());
    let tool = named
        .or_else(|| std::env::var_os("PAPERPAW_SIGN").map(PathBuf::from))
        .or(beside)
        .unwrap_or_else(|| PathBuf::from(&name));
    if Command::new(&tool).arg("--help").output().is_ok() {
        return Ok(tool);
    }
    Err(format!(
        "the packer `paperpaw-sign` was not found ({}). It turns the built program into the \
         plugin's file and comes with PaperPaw, as a program of its own. Install it beside \
         paperpaw-dev:\n    \
         cargo install --locked --git {REPOSITORY} --tag v{PAPERPAW} paperpaw-sign\n\
         or, from a PaperPaw checkout:\n    \
         cargo install --locked --path tools/sign\n\
         or name the program with `--sign-tool <path>` or in the variable PAPERPAW_SIGN.",
        tool.display()
    ))
}

/// Runs `command`, its output shown as it comes; `what` names it in the
/// error.
fn step(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command
        .env_remove("RUSTFLAGS")
        .status()
        .map_err(|e| format!("{what} could not be started: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed: what it said is above"))
    }
}

/// Copies the folder `from` with all in it to `to`.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    let failed = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(to).map_err(|e| failed(to, e))?;
    for entry in std::fs::read_dir(from).map_err(|e| failed(from, e))? {
        let entry = entry.map_err(|e| failed(from, e))?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy(&source, &target)?;
        } else {
            std::fs::copy(&source, &target).map_err(|e| failed(&source, e))?;
        }
    }
    Ok(())
}

/// `build [<plugin's folder>] [--device <device>] [--sign-tool <path>]`.
///
/// # Errors
/// A wrong option, a compiler or packer that is missing (with how to get
/// it), a build that fails, or a program the packer refuses.
pub fn run(args: &[String]) -> Result<bool, String> {
    let (mut dir, mut chip, mut tool) = (PathBuf::from("."), &X4, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => {
                chip = match args.next().map(String::as_str) {
                    Some("x4" | "x3") => &X4,
                    Some("x4-pro" | "x4-classic") => &X4_PRO,
                    _ => return Err("--device is one of x4, x3, x4-pro, x4-classic".into()),
                }
            }
            "--sign-tool" => {
                tool = Some(PathBuf::from(
                    args.next().ok_or("--sign-tool needs a path")?,
                ));
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => dir = PathBuf::from(other),
        }
    }
    let id = package(&dir)?;
    // The packer first: without it nothing below ends in a file.
    let tool = sign_tool(tool)?;
    crate::metadata::write(&dir, false)?;
    let build_std = compiler(&dir, chip)?;

    let mut cargo = Command::new("cargo");
    cargo.current_dir(&dir);
    if let Some(toolchain) = chip.toolchain {
        cargo.arg(format!("+{toolchain}"));
    }
    cargo.args(["build", "--release", "--target", chip.triple]);
    if build_std {
        cargo.arg("-Zbuild-std=core,alloc");
    }
    step(&mut cargo, "the build (cargo)")?;

    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| dir.join("target"), PathBuf::from);
    let built = target.join(chip.triple).join("release").join(&id);
    // The folder as it goes onto the card: what `plugin` holds, and the code.
    let folder = dir.join("dist").join(chip.device).join(&id);
    let _ = std::fs::remove_dir_all(&folder);
    copy(&dir.join(FOLDER), &folder)?;
    // What only the plugin store reads does not go onto a card.
    let _ = std::fs::remove_dir_all(folder.join("store"));
    let file = folder.join(FILE);
    step(
        Command::new(&tool).arg("pack").arg(&built).arg(&file),
        "the packer (paperpaw-sign pack)",
    )?;
    step(
        Command::new(&tool).arg("plugin").arg(&file),
        "the check of the packed file (paperpaw-sign plugin)",
    )?;
    println!(
        "\n{} is the plugin for {} (`--device {}`).\n\
         To try it, copy that folder into the folder `plugins` of the card and start the \
         device. Its code is not signed: a device runs an unsigned plugin from the card, and \
         the plugin store signs what it publishes.",
        folder.display(),
        chip.devices,
        chip.device
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_packer_that_is_missing_is_said_with_how_to_install_it() {
        let error = sign_tool(Some(PathBuf::from("/nowhere/paperpaw-sign"))).unwrap_err();
        assert!(error.contains("cargo install --locked --git"), "{error}");
        assert!(
            error.contains(&format!("--tag v{PAPERPAW} paperpaw-sign")),
            "{error}"
        );
        assert!(error.contains("--sign-tool <path>"), "{error}");
    }

    #[test]
    fn the_crates_name_is_read_from_its_package_section() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-build-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\n\n[package]\nname = \"night-owl\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"other\"\n",
        )
        .unwrap();
        assert_eq!(package(&dir).unwrap(), "night-owl");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            package(&dir)
                .unwrap_err()
                .contains("new --github <id> --code")
        );
    }
}
