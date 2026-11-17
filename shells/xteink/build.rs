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
    println!("cargo::rerun-if-changed=rwtext-x4.x");
    println!("cargo::rerun-if-changed=wifi-flash-x4.x");
    plugins();
    device_messages();
    compiler();
    frame_flags();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    if std::env::var_os("CARGO_FEATURE_ESP32S3").is_some() {
        println!("cargo::rustc-link-arg=-nostartfiles");
        println!("cargo::rustc-link-arg=-Wl,-Tlinkall.x");
        stack_floor(&out);
        no_rodata_gap(&out);
        return;
    }
    std::fs::copy("memory-x4.x", out.join("memory.x")).expect("memory-x4.x");
    // esp-hal's `rwtext.x` less the Wi-Fi send and receive code, which
    // `wifi-flash-x4.x` puts in flash (found first as it is in `out`).
    std::fs::copy("rwtext-x4.x", out.join("rwtext.x")).expect("rwtext-x4.x");
    std::fs::copy("wifi-flash-x4.x", out.join("wifi-flash-x4.x")).expect("wifi-flash-x4.x");
    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rustc-link-arg=-Tlinkall.x");
    println!("cargo::rustc-link-arg=-Twifi-flash-x4.x");
    no_rodata_gap(&out);
    if std::env::var_os("CARGO_FEATURE_PPN5").is_some() {
        stack_floor_x4(&out);
    }
}

/// The least main stack of the X4 where interface-7 plugins are built in
/// (`ppn5`): as the ESP32-S3's (`STACK_FLOOR_BAND`): the deepest path measured
/// (a plugin's call into the files host and the FAT reached 37.9 KB on the
/// device, over the band of the first test images, #2556) and the band with its
/// reserve at the stack's end (7.7 KB). Statics that grow shrink the stack
/// unseen, so the link fails below this instead.
const STACK_FLOOR_X4_BAND: u32 = STACK_FLOOR_BAND;

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

/// Fails the link where a gap is left between the application descriptor's
/// section (with the plugin list: `plugins()` pads it) and `.rodata`: esp-hal's
/// script pads there (`.rodata_merge`) to the alignment `.rodata` asks for,
/// and `espflash` then makes two data segments of the image, of which the
/// ESP32-S3's bootloader maps the last only: the firmware would read its
/// own plugin list from the wrong flash page (`paperpaw-sign check-image`
/// finds the two segments in an image). Where `.rodata` asks for more than
/// the list's padding (`paperpaw_update::Plugins::record`, 64 bytes) keeps, the link
/// fails here, at once, and the padding is raised.
fn no_rodata_gap(out: &std::path::Path) {
    let script = out.join("rodata-gap.x");
    // By addresses, not by the gap section's size: a renamed or dropped
    // `.rodata_merge` would read 0 and pass; a missing section named here
    // fails the link instead.
    let text = "ASSERT(ADDR(.rodata) == ADDR(.flash.appdesc) + SIZEOF(.flash.appdesc), \
                \"a gap is left between the application descriptor's section and \
                .rodata: raise SECTION_END_ALIGN in crates/paperpaw-update/src/plugins.rs\");\n";
    std::fs::write(&script, text).expect("rodata-gap.x");
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

/// Writes `device_messages.rs`: the device's texts (`i18n/*.ini`), English and
/// German, without their keys (`paperpaw_i18n::BuiltMessages`, as the
/// plugins' and the kernel's are): the firmware carries a hash of each key
/// and the messages, not the sources (#306). `device.rs` hands them to the
/// builder.
fn device_messages() {
    let mut table = paperpaw_i18n::BuiltMessagesTable::new(&["en", "de"]);
    for file in ["i18n/en.ini", "i18n/de.ini"] {
        println!("cargo::rerun-if-changed={file}");
        let source = std::fs::read_to_string(file).expect("a catalog");
        assert!(
            table.add(&source).unwrap_or(false),
            "{file}: a device text catalog the keyless table cannot hold: {:?}",
            table.clashes()
        );
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("device_messages.rs"), table.to_rust())
        .expect("device_messages.rs written");
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
