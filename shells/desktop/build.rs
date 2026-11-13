//! Tells the emulator the commit of PaperPaw's source it is built from,
//! and whether that commit is the release of its version (tagged
//! `v<version>`, with nothing changed in the tracked files), for
//! `--stdio`'s `ready`: tools pin `paperpaw-sdk` to exactly that source. `PAPERPAW_COMMIT` and `PAPERPAW_RELEASE` (`true`
//! or `false`) say it where there is no Git (the release workflow sets
//! them); without either, neither is known.

use std::process::Command;

/// What `git` says in this folder, trimmed; `None` where it says nothing.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// Whether the tracked files are as committed; not where Git cannot say.
fn clean() -> bool {
    Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .is_ok_and(|o| o.status.success() && o.stdout.iter().all(u8::is_ascii_whitespace))
}

/// Whether this build hosts the linked native plugins: the `native-linked`
/// feature on a target interface 7 declares a profile for and the shell can
/// run on (Decision 172: Linux and Windows on x86-64, macOS on x86-64 and
/// ARM, the browser on wasm32). `cfg(linked_host)` in the sources says so. The same targets are
/// listed in `Cargo.toml` (the `paperpaw-app` dependency): change both together.
fn linked_host() {
    println!("cargo::rustc-check-cfg=cfg(linked_host)");
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    let (os, arch) = (var("CARGO_CFG_TARGET_OS"), var("CARGO_CFG_TARGET_ARCH"));
    let target_ok = matches!(
        (os.as_str(), arch.as_str()),
        ("linux" | "windows", "x86_64") | ("macos", "x86_64" | "aarch64") | ("unknown", "wasm32")
    );
    if target_ok && std::env::var_os("CARGO_FEATURE_NATIVE_LINKED").is_some() {
        println!("cargo::rustc-cfg=linked_host");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    linked_host();
    metadata_constants()?;
    println!("cargo::rerun-if-env-changed=PAPERPAW_COMMIT");
    println!("cargo::rerun-if-env-changed=PAPERPAW_RELEASE");
    let tag = format!(
        "v{}",
        std::env::var("CARGO_PKG_VERSION").unwrap_or_default()
    );
    let commit = std::env::var("PAPERPAW_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]));
    let release = match std::env::var("PAPERPAW_RELEASE").ok().as_deref() {
        Some("true") => true,
        Some(_) => false,
        None => {
            let tagged = git(&["tag", "--points-at", "HEAD"])
                .is_some_and(|tags| tags.lines().any(|t| t == tag));
            // At the tag, a change to a file makes it no release: looked
            // at in every build there, as Cargo cannot watch every file.
            if tagged {
                println!("cargo::rerun-if-changed=.always-look-again");
            }
            tagged && clean()
        }
    };
    // Built again when the checkout moves on or is tagged; only what is
    // there (a path that is not makes Cargo build again every time).
    let mut watched = vec![
        String::from("HEAD"),
        String::from("packed-refs"),
        String::from("refs/tags"),
    ];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &name])
            && std::path::Path::new(&path).exists()
        {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    println!(
        "cargo::rustc-env=PAPERPAW_SOURCE_COMMIT={}",
        commit.unwrap_or_default()
    );
    println!("cargo::rustc-env=PAPERPAW_SOURCE_RELEASE={release}");
    // The compiler code plugins for this computer must be built with
    // (the SDK, without a build script, cannot know it): `--stdio`'s
    // `ready` says it.
    println!("cargo::rerun-if-env-changed=RUSTC");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| String::from("rustc"));
    let words = Command::new(rustc)
        .arg("-vV")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let first = words.lines().next().unwrap_or("rustc").trim();
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo::rustc-env=PAPERPAW_HOST_TOOLCHAIN={first}, {target}");
    // And by what a plugin's library tells of its compiler: the release
    // an ELF library names, the commit the paths of `std` name.
    let release = paperpaw_native::compiler::release(first).unwrap_or_default();
    println!("cargo::rustc-env=PAPERPAW_RUSTC={release}");
    let commit = words
        .lines()
        .find_map(|l| l.strip_prefix("commit-hash: "))
        .unwrap_or_default();
    println!("cargo::rustc-env=PAPERPAW_RUSTC_COMMIT={}", commit.trim());
    Ok(())
}

// Offsets and profile values come only from the generated descriptor.
fn metadata_constants() -> Result<(), Box<dyn std::error::Error>> {
    let path = "../../abi/generated/metadata-layout.json";
    println!("cargo::rerun-if-changed={path}");
    let descriptor: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let mut source = String::new();
    for field in descriptor["prefix"]["fields"]
        .as_array()
        .ok_or("descriptor field must be an array")?
    {
        source += &format!(
            "pub const {}: usize = {};\n",
            field["name"]
                .as_str()
                .ok_or("descriptor field must be text")?
                .to_uppercase(),
            field["offset"]
        );
    }
    source += &format!(
        "pub const PREFIX_SIZE: usize = {};\n",
        descriptor["prefix"]["size"]
    );
    source += &format!(
        "pub const FORMAT: u16 = {};\n",
        descriptor["descriptor"]["format"]
    );
    source += &format!(
        "pub const MAX_BYTES: usize = {};\n",
        descriptor["descriptor"]["max_bytes"]
    );
    source += &format!(
        "pub const MAGIC_BYTES: &[u8] = b{:?};\n",
        descriptor["descriptor"]["magic"]
            .as_str()
            .ok_or("descriptor field must be text")?
    );
    let profile = descriptor["descriptor"]["profiles"]
        .as_array()
        .ok_or("descriptor field must be an array")?
        .iter()
        .find(|p| p["name"] == "linux-x86_64-sysv64-le")
        .ok_or("descriptor must contain the reviewed Linux x86-64 profile")?;
    source += &format!(
        "pub const PROFILE: &[u8] = b{:?};\n",
        profile["name"]
            .as_str()
            .ok_or("descriptor field must be text")?
    );
    for key in ["entry_layout", "entry_bytes", "entry_align"] {
        source += &format!(
            "pub const PROFILE_{}: usize = {};\n",
            key.to_uppercase(),
            profile[key]
        );
    }
    for field in descriptor["records"]["export"]["fields"]
        .as_array()
        .ok_or("descriptor field must be an array")?
    {
        source += &format!(
            "pub const EXPORT_{}: usize = {};\n",
            field["name"]
                .as_str()
                .ok_or("descriptor field must be text")?
                .to_uppercase(),
            field["offset"]
        );
    }
    source += &format!(
        "pub const EXPORT_SIZE: usize = {};\n",
        descriptor["records"]["export"]["size"]
    );
    let out =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo must supply OUT_DIR")?);
    std::fs::write(out.join("metadata.rs"), source)?;
    Ok(())
}
