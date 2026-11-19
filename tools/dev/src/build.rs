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
//! it). `--device emulator` builds the plugin for this computer instead
//! (`build/emulator.rs`).
//!
//! The same source makes the same file in any folder and on any computer
//! (the plugin store rebuilds a published plugin and compares): the paths
//! that end up in a build, in the messages of its panics and the lines of
//! its crashes, are written as `/plugin` (the plugin's folder), `/cargo`
//! (Cargo's home, where its dependencies lie), `/paperpaw` (a PaperPaw
//! checkout the SDK is taken from by a path), `/target` (a build folder
//! elsewhere) and `/rust` (the compiler, where its libraries are built
//! along), whatever they are here (`same_paths`).

mod emulator;

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
/// This computer: the library the emulator loads ([`emulator`]).
const EMULATOR: Chip = Chip {
    device: "emulator",
    devices: "the emulator on this computer",
    triple: "x86_64-unknown-linux-gnu",
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
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
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

/// The folder the SDK is taken from where `Cargo.toml` names it by a path
/// outside the plugin's folder (`paperpaw-sdk = { path = '/..' }`): the
/// PaperPaw checkout it lies in (`crates/paperpaw-sdk` of it), else itself.
/// Its path is kept out of the build, but such a build is still not the same
/// on another computer (Cargo makes the path a part of it): `new --code
/// --sdk` names the SDK through a link in the plugin's folder instead.
fn sdk_folder(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let value = text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("paperpaw-sdk")?.trim_start();
        let table = rest.strip_prefix('=')?.trim().strip_prefix('{')?;
        let path = table.trim_start().strip_prefix("path")?.trim_start();
        let path = path.strip_prefix('=')?.trim_start();
        let quote = path.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
        path[1..].split(quote).next()
    })?;
    // One in the plugin's own folder (its link `.paperpaw`) is written as
    // the folder is.
    if Path::new(value).is_relative() {
        return None;
    }
    let sdk = dir.join(value).canonicalize().ok()?;
    let checkout = sdk
        .parent()
        .filter(|crates| sdk.ends_with("crates/paperpaw-sdk") && crates.parent().is_some())
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    Some(checkout.unwrap_or(sdk))
}

/// Cargo's setting that has the compiler write the folders of this build
/// the same wherever they lie (the module's description), for `triple`:
/// added to the flags of the plugin's `.cargo/config.toml`, which link it.
/// `rust` is the compiler's folder, where its libraries are built along.
fn same_paths(dir: &Path, target: &Path, triple: &str, rust: Option<&Path>) -> String {
    let whole = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let source = whole(dir);
    let cargo = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            Some(PathBuf::from(home).join(".cargo"))
        })
        .map(|home| whole(&home));
    let target = Some(whole(target)).filter(|target| !target.starts_with(&source));
    // The compiler takes the last of these that fits a path: the folder of
    // the plugin before what may lie in it.
    let folders = [
        (cargo, "/cargo"),
        (Some(source), "/plugin"),
        (target, "/target"),
        (sdk_folder(dir), "/paperpaw"),
        (rust.map(whole), "/rust"),
    ];
    let flags: Vec<String> = folders
        .into_iter()
        .filter_map(|(folder, name)| {
            // A TOML string: `\` and `"` are written with a `\` before them.
            let folder = folder?.display().to_string();
            let folder = folder.replace('\\', "\\\\").replace('"', "\\\"");
            Some(format!("\"--remap-path-prefix={folder}={name}\""))
        })
        .collect();
    format!("target.{triple}.rustflags=[{}]", flags.join(", "))
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

/// The plugin's id: the crate's name, held to the rule `new --code` holds an
/// id to before anything is named, made or removed by it (`dist/<device>/<id>`).
fn id_of(dir: &Path) -> Result<String, String> {
    let id = package(dir)?;
    let rules = paperpaw_publish::Rules::built_in();
    if rules.valid_id(&id) && id.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Ok(id);
    }
    Err(format!(
        "{}: the package's name `{id}` is no plugin id (lower-case letters, digits and \
         hyphens, beginning with a letter, at most {}; nothing after it on its line). The \
         plugin's folder is named by it",
        dir.join("Cargo.toml").display(),
        rules.max_id
    ))
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
    let install = format!(
        "Install it beside paperpaw-dev:\n    \
         cargo install --locked --git {REPOSITORY} --tag v{PAPERPAW} paperpaw-sign\n\
         or, from a PaperPaw checkout:\n    \
         cargo install --locked --path tools/sign\n\
         or name the program with `--sign-tool <path>` or in the variable PAPERPAW_SIGN."
    );
    let Ok(output) = Command::new(&tool).arg("--version").output() else {
        return Err(format!(
            "the packer `paperpaw-sign` was not found ({}). It turns the built program into \
             the plugin's file and comes with PaperPaw, as a program of its own. {install}",
            tool.display()
        ));
    };
    same_release(&String::from_utf8_lossy(&output.stdout)).map_err(|is| {
        format!(
            "the packer {} is {is}, this paperpaw-dev is of PaperPaw {PAPERPAW}: a plugin's \
             file is packed by the release it is built for. {install}",
            tool.display()
        )
    })?;
    Ok(tool)
}

/// Whether what `paperpaw-sign --version` printed names this tool's release;
/// else what the packer is, for the message.
fn same_release(printed: &str) -> Result<(), String> {
    match printed.trim().strip_prefix("paperpaw-sign ") {
        Some(version) if version == PAPERPAW => Ok(()),
        Some(version) => Err(format!("of PaperPaw {version}")),
        // One from before it knew `--version`.
        None => Err("of an older PaperPaw".to_string()),
    }
}

/// Runs `command`, its output shown as it comes; `what` names it in the
/// error.
fn step(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
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
                    Some("emulator") => &EMULATOR,
                    _ => {
                        return Err(
                            "--device is one of x4, x3, x4-pro, x4-classic, emulator".into()
                        );
                    }
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
    let id = id_of(&dir)?;
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| dir.join("target"), PathBuf::from);
    if chip.device == EMULATOR.device {
        crate::metadata::write(&dir, false)?;
        compiler(&dir, chip)?;
        return emulator::build(&dir, &id, &target);
    }
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
    let mut rust = None;
    if build_std {
        cargo.arg("-Zbuild-std=core,alloc");
        let toolchain = format!("+{}", chip.toolchain.unwrap_or("stable"));
        rust = said(&dir, "rustc", &[&toolchain, "--print", "sysroot"])
            .map(|sysroot| PathBuf::from(sysroot.trim()));
    }
    cargo
        .arg("--config")
        .arg(same_paths(&dir, &target, chip.triple, rust.as_deref()));
    step(&mut cargo, "the build (cargo)")?;

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
         It is the plugin's folder as it lies in the folder `plugins` of a card. Its code is \
         not signed.",
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
    fn a_packer_of_another_release_is_told_apart() {
        assert_eq!(same_release(&format!("paperpaw-sign {PAPERPAW}\n")), Ok(()));
        assert_eq!(
            same_release("paperpaw-sign 0.0.1\n"),
            Err("of PaperPaw 0.0.1".to_string())
        );
        assert!(same_release("unexpected arguments").is_err());
    }

    /// A package name that is no plugin id never reaches a path: `dist/<id>`
    /// is removed and made anew by the build.
    #[test]
    fn a_crate_name_that_is_no_plugin_id_is_refused() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-id-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let named = |line: &str| {
            std::fs::write(dir.join("Cargo.toml"), format!("[package]\n{line}\n")).unwrap();
            id_of(&dir)
        };
        assert_eq!(named("name = \"night-owl\"").unwrap(), "night-owl");
        for line in [
            "name = \"..\"",
            "name = \"../../elsewhere\"",
            "name = \"a/b\"",
            "name = \"\"",
            "name = \"night-owl\" # the plugin",
            "name = \"Night\"",
            "name = \"7-up\"",
        ] {
            let error = named(line).unwrap_err();
            assert!(error.contains("is no plugin id"), "{line}: {error}");
        }
        let _ = std::fs::remove_dir_all(&dir);
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
