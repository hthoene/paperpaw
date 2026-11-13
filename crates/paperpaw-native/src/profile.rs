//! Target profiles: the one a program is built for, and a plugin's metadata
//! set to it (Decision 172: linked native plugins on every shell). Always
//! built: the SDK sets a linked plugin's metadata with it, the shells host
//! with it (`metadata` re-exports it).
//!
//! A linked plugin is compiled into the program that hosts it, so its entry
//! has the program's own layout. Its metadata names that target's declared
//! profile (`abi/abi.def`), chosen here by the compiler's `cfg`, never by a
//! shell: a build for a target with no declared profile has none
//! ([`BUILD_PROFILE`] is `None`), and the SDK then refuses to link a plugin.

/// The shell's reviewed target profile, independent of the build machine.
#[derive(Clone, Copy, Debug)]
pub struct Profile<'a> {
    pub target: &'a str,
    pub entry_layout: u16,
    pub entry_bytes: u16,
}

/// The format-2 prefix and the largest record (`metadata::MAX_BYTES`).
pub(crate) const PREFIX_SIZE: usize = 32;
pub(crate) const MAX_BYTES: usize = 2048;

/// A declared profile with entry layout 1 (`PpEntry`).
const fn profile(target: &'static str, entry_bytes: u16) -> Option<Profile<'static>> {
    Some(Profile {
        target,
        entry_layout: 1,
        entry_bytes,
    })
}

/// The declared profile (`abi/abi.def`) of the target this program is built
/// for: what a linked plugin's metadata names, and what the shell that hosts
/// it checks. `None` where interface 7 declares no profile for the target.
pub const BUILD_PROFILE: Option<Profile<'static>> = if cfg!(not(target_endian = "little")) {
    None
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    profile("linux-x86_64-sysv64-le", 32)
} else if cfg!(all(target_os = "none", target_arch = "riscv32")) {
    profile("esp32c3-riscv32imc-ilp32-soft-le", 20)
} else if cfg!(all(target_os = "none", target_arch = "xtensa")) {
    profile("esp32s3-xtensa-lx7-ilp32-windowed-le", 20)
} else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    profile("windows-x86_64-win64-le", 32)
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    profile("macos-x86_64-sysv64-le", 32)
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    profile("macos-aarch64-darwinpcs-le", 32)
} else if cfg!(all(target_os = "android", target_arch = "aarch64")) {
    profile("android-aarch64-aapcs64-le", 32)
} else if cfg!(all(target_os = "unknown", target_arch = "wasm32")) {
    profile("web-wasm32-c-le", 20)
} else {
    None
};

/// Offsets in the format-2 prefix (`abi/generated/metadata-layout.json`).
const TOTAL: usize = 8;
const ENTRY_LAYOUT: usize = 16;
const ENTRY_BYTES: usize = 18;
const TARGET_BYTES: usize = 24;
const EXPORTS: usize = 30;
/// The entry export's size field, after its total and kind.
const EXPORT_ENTRY_BYTES: usize = 4;

const fn u16_at(bytes: &[u8], at: usize) -> usize {
    u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize
}

/// Whether `metadata` has a format-2 prefix of its own length, one export
/// and room for its target name and that export.
const fn framed(metadata: &[u8]) -> bool {
    if metadata.len() < PREFIX_SIZE
        || metadata[0] != b'P'
        || metadata[1] != b'P'
        || metadata[2] != b'A'
        || metadata[3] != b'M'
        || u16_at(metadata, 4) != 2
        || u16_at(metadata, 6) != PREFIX_SIZE
        || u16_at(metadata, EXPORTS) != 1
    {
        return false;
    }
    let total = u32::from_le_bytes([
        metadata[TOTAL],
        metadata[TOTAL + 1],
        metadata[TOTAL + 2],
        metadata[TOTAL + 3],
    ]);
    let export = PREFIX_SIZE + u16_at(metadata, TARGET_BYTES);
    total as usize == metadata.len()
        && export + EXPORT_ENTRY_BYTES + 4 <= metadata.len()
        && metadata[export + 2] == 1
        && metadata[export + 3] == 0
}

/// The length of `metadata` set to `profile` ([`retarget`]).
///
/// # Panics
/// Where `metadata` is not a format-2 record with one export (at compile
/// time when called in a constant).
#[must_use]
pub const fn retargeted_len(metadata: &[u8], profile: &Profile<'_>) -> usize {
    assert!(
        framed(metadata),
        "not interface-7 metadata (format 2, one export)"
    );
    metadata.len() - u16_at(metadata, TARGET_BYTES) + profile.target.len()
}

/// `metadata` (format 2, one export) naming `profile` instead of the profile
/// it was generated for: its target name, entry layout and entry size, in
/// the prefix and in the entry export; the services and features unchanged.
/// `N` is [`retargeted_len`].
///
/// # Panics
/// Where `metadata` is not such a record or `N` is not its new length (at
/// compile time when called in a constant).
#[must_use]
pub const fn retarget<const N: usize>(metadata: &[u8], profile: &Profile<'_>) -> [u8; N] {
    assert!(
        N == retargeted_len(metadata, profile),
        "the retargeted metadata has another length"
    );
    assert!(N <= MAX_BYTES, "the retargeted metadata is too long");
    let target = profile.target.as_bytes();
    let mut out = [0; N];
    let mut i = 0;
    while i < PREFIX_SIZE {
        out[i] = metadata[i];
        i += 1;
    }
    let total = (N as u32).to_le_bytes();
    let mut k = 0;
    while k < 4 {
        out[TOTAL + k] = total[k];
        k += 1;
    }
    let layout = profile.entry_layout.to_le_bytes();
    let entry = profile.entry_bytes.to_le_bytes();
    let target_len = (target.len() as u16).to_le_bytes();
    out[ENTRY_LAYOUT] = layout[0];
    out[ENTRY_LAYOUT + 1] = layout[1];
    out[ENTRY_BYTES] = entry[0];
    out[ENTRY_BYTES + 1] = entry[1];
    out[TARGET_BYTES] = target_len[0];
    out[TARGET_BYTES + 1] = target_len[1];
    let mut t = 0;
    while t < target.len() {
        out[PREFIX_SIZE + t] = target[t];
        t += 1;
    }
    // The export and everything after it, moved by the change of name length.
    let mut from = PREFIX_SIZE + u16_at(metadata, TARGET_BYTES);
    let export = PREFIX_SIZE + target.len();
    let mut to = export;
    while from < metadata.len() {
        out[to] = metadata[from];
        from += 1;
        to += 1;
    }
    let size = (profile.entry_bytes as u32).to_le_bytes();
    let mut s = 0;
    while s < 4 {
        out[export + EXPORT_ENTRY_BYTES + s] = size[s];
        s += 1;
    }
    out
}
