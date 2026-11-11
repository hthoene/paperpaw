#![expect(
    clippy::expect_used,
    reason = "a build script that cannot go on fails at once"
)]

//! Links with esp-hal's linker script. Here rather than in
//! `.cargo/config.toml`: `RUSTFLAGS` (as CI sets it) replaces the
//! configuration's flags, not this.
//!
//! The X4's memory map is PaperPaw's own (`memory-x4.x`, copied as
//! `memory.x`; not named so here, where the linker would find it for the
//! X4 Pro too): esp-hal's maps 4 MB
//! of flash for code and read-only data together, and the fonts take much
//! of it. The X4 Pro and X4 Classic (ESP32-S3) keep esp-hal's, whose code and data each
//! have their own 32 MB window; its linker (GCC) takes the script through
//! `-Wl`.
//!
//! Also gives the firmware PaperPaw's version (`PAPERPAW_VERSION`), the
//! workspace's: the firmware is outside it; and the list of the plugin
//! crates built in (`plugins.rs`, see `paperpaw_update::Plugins`): those
//! of the app as built here (`paperpaw-app`'s, by `links`), with their
//! versions from the firmware's lockfile, which names optional crates
//! whether they are built or not. And the compiler it is built with
//! (`PAPERPAW_RUSTC`): code plugins run only where it built them too.

use std::path::PathBuf;

fn main() {
    let workspace = std::fs::read_to_string("../../Cargo.toml").expect("../../Cargo.toml");
    let version = workspace
        .split("[workspace.package]")
        .nth(1)
        .and_then(|part| part.lines().find_map(|l| l.strip_prefix("version = ")))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("the workspace's version");
    println!("cargo::rustc-env=PAPERPAW_VERSION={version}");
    println!("cargo::rerun-if-changed=../../Cargo.toml");
    println!("cargo::rerun-if-changed=memory-x4.x");
    plugins();
    compiler();
    frame_flags();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    if std::env::var_os("CARGO_FEATURE_ESP32S3").is_some() {
        println!("cargo::rustc-link-arg=-nostartfiles");
        println!("cargo::rustc-link-arg=-Wl,-Tlinkall.x");
        stack_floor(&out);
        return;
    }
    std::fs::copy("memory-x4.x", out.join("memory.x")).expect("memory-x4.x");
    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rustc-link-arg=-Tlinkall.x");
    if std::env::var_os("CARGO_FEATURE_PPN5").is_some() {
        stack_floor_x4(&out);
    }
}

/// The least main stack of the X4 where interface-7 plugins are built in
/// (`ppn5`): the deepest path of the main loop (some 35.8 KB: `main`, the
/// firmware's run and a plugin's install with its signature check) and the
/// band with its reserve at the stack's end (7.7 KB), 43.5 KB. The first test
/// image had 37.4 KB and tripped the band at the start (#2556); statics that
/// grow shrink the stack unseen, so the link fails below this instead.
const STACK_FLOOR_X4_BAND: u32 = 43 * 1024 + 512;

/// Fails the link where the X4's main stack is below [`STACK_FLOOR_X4_BAND`]:
/// a linker script of its own, read beside esp-hal's.
fn stack_floor_x4(out: &std::path::Path) {
    let script = out.join("stack-floor.x");
    let floor = STACK_FLOOR_X4_BAND;
    let text = format!(
        "ASSERT(_stack_start - _stack_end >= {floor}, \"the main stack is below \
         {floor} bytes: give the heap less (HEAP_SIZE, X4_BAND_KB)\");\n"
    );
    std::fs::write(&script, text).expect("stack-floor.x");
    println!("cargo::rustc-link-arg={}", script.display());
}

/// The frame budgets are measured with frame pointers (the flags of
/// `.cargo/config.toml`). A `RUSTFLAGS` in the environment (CI sets
/// `-D warnings`) replaces the config's flags, and the build would then
/// differ from the one the budgets were checked on, so it fails here instead.
fn frame_flags() {
    println!("cargo::rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("riscv32") {
        return;
    }
    let flags = std::env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let flags: Vec<&str> = flags.split('\x1f').collect();
    let has = |wanted: &str| {
        flags.windows(2).any(|pair| pair == ["-C", wanted])
            || flags.contains(&format!("-C{wanted}").as_str())
    };
    assert!(
        has("force-frame-pointers"),
        "the X4 builds with `-C force-frame-pointers` \
         (shells/xteink/.cargo/config.toml); a RUSTFLAGS in the environment \
         replaces it. Build with `env -u RUSTFLAGS cargo ...`."
    );
}

/// The least main stack the ESP32-S3 devices are linked with. Their IRAM
/// and DRAM are one SRAM and the stack gets what is left, so code moved
/// to IRAM (Bluetooth's controller takes 21 KB) shrinks it unseen, and
/// the firmware then overruns it at start. The deepest use measured is
/// some 32 KB (a book's first opening, ADR 0017); the X4 Pro has 41.3 KB,
/// with Bluetooth too (its heap is smaller by as much as Bluetooth takes).
const STACK_FLOOR: u32 = 40 * 1024;

/// The least main stack where interface-7 plugins are built in (`ppn5`): the
/// deepest path measured (some 33 KiB) and the band with its reserve
/// (`paperpaw_x4::s3_band::STACK_FLOOR`, whose test reads this line).
const STACK_FLOOR_BAND: u32 = 50 * 1024;

/// Fails the link where the main stack is below [`STACK_FLOOR`]: a
/// linker script of its own, read beside esp-hal's.
fn stack_floor(out: &std::path::Path) {
    let script = out.join("stack-floor.x");
    let floor = if std::env::var_os("CARGO_FEATURE_PPN5").is_some() {
        STACK_FLOOR_BAND
    } else {
        STACK_FLOOR
    };
    let text = format!(
        "ASSERT(_stack_start - _stack_end >= {floor}, \"the main stack is below \
         {floor} bytes: give the heap less (HEAP_SIZE)\");\n"
    );
    std::fs::write(&script, text).expect("stack-floor.x");
    println!("cargo::rustc-link-arg={}", script.display());
}

/// Gives the firmware the release of the compiler that builds it, as
/// plugin files name theirs (`paperpaw_native::compiler::release`).
fn compiler() {
    let rustc = std::env::var_os("RUSTC").expect("RUSTC");
    let out = std::process::Command::new(rustc)
        .arg("-vV")
        .output()
        .expect("rustc -vV");
    let words = String::from_utf8(out.stdout).expect("rustc -vV says text");
    let first = words.lines().next().unwrap_or_default();
    let release = paperpaw_native::compiler::release(first).expect("rustc -vV names its release");
    println!("cargo::rustc-env=PAPERPAW_RUSTC={release}");
}

/// Writes `plugins.rs`: the list of the plugin crates built in, the
/// bytes the firmware keeps right after its application descriptor.
fn plugins() {
    println!("cargo::rerun-if-changed=Cargo.lock");
    let built: Vec<String> = std::env::var("DEP_PAPERPAW_APP_CRATES")
        .expect("paperpaw-app names its plugin crates")
        .split(',')
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect();
    let lock = std::fs::read_to_string("Cargo.lock").expect("Cargo.lock");
    let mut crates: Vec<(String, String)> = Vec::new();
    for package in lock.split("[[package]]") {
        let field = |key: &str| {
            package.lines().find_map(|l| {
                l.strip_prefix(key)
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
        };
        if let (Some(name), Some(version)) = (field("name = "), field("version = "))
            && built.contains(&name)
        {
            crates.push((name, version));
        }
    }
    crates.sort();
    let bytes = paperpaw_update::Plugins::record(
        crates
            .iter()
            .map(|(name, version)| paperpaw_update::PluginCrate { name, version }),
    )
    .expect("the list of plugins is longer than updates and the web flasher read");
    let code = format!(
        "/// The plugin crates built in (`paperpaw_update::Plugins`), right after
         /// the application descriptor, where updates and the web flasher read them.
         #[unsafe(link_section = \".flash.appdesc.plugins\")]
         #[used]
         pub static PLUGINS: [u8; {}] = {:?};
",
        bytes.len(),
        bytes
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("plugins.rs"), code).expect("plugins.rs");
}
