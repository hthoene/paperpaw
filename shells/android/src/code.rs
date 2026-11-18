//! Android loads no code plugins (ADR 0024, amendment of 2026-11, #228).
//!
//! The kernel lists every plugin folder that holds a code file (`code.so`,
//! `code.ppn`, ...) and loads its code only where the shell gave it a loader
//! (`KernelBuilder::code_plugins`). The Android shell gives none, so such a
//! plugin is listed as failed with [`PluginProblem::Unsupported`] ("this
//! PaperPaw loads no code plugins") and no file is opened, mapped or run, whatever
//! interface its library is built for. Data plugins load as before.
//!
//! [`policy`] is the one place where that is decided; the tests below fail if
//! it ever installs a loader.

use paperpaw_core::KernelBuilder;

/// The code-plugin policy of the Android kernel: none. Returns `builder`
/// as it is; there is deliberately no `code_plugins` call.
#[must_use]
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "the Android app calls it; on a computer only the tests do"
    )
)]
pub(crate) fn policy(builder: KernelBuilder) -> KernelBuilder {
    builder
}

#[cfg(test)]
mod tests {
    use paperpaw_core::Kernel;
    use paperpaw_platform::{
        DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation,
    };
    use paperpaw_sdk::{MemoryStorage, PluginKind, PluginProblem, PluginState, Storage};

    use super::policy;

    fn device() -> DeviceInfo {
        DeviceInfo::new(
            "android",
            "Android e-reader",
            DisplayInfo::new(480, 800, 220, PixelFormat::Gray8, Rotation::Deg0, None),
            InputCaps::new(true, true, false, &[]),
            MemoryClass::Large,
        )
    }

    /// A card with a data-only plugin and code plugins of every file name a
    /// library or an image can have, the interface-7 `.so` among them. The
    /// bytes are junk on purpose: nothing may open them.
    fn card() -> MemoryStorage {
        let storage = MemoryStorage::new();
        let plugins = [
            ("data", None),
            ("seven", Some("code.so")),
            ("six", Some("code-host.so")),
            ("x4", Some("code.ppn")),
        ];
        for (id, code) in plugins {
            let _ = storage.create_dir(&format!("plugins/{id}"));
            let ini = format!("[plugin]\nname = {id}\nid = {id}\n");
            storage
                .write(&format!("plugins/{id}/plugin.ini"), ini.as_bytes())
                .unwrap();
            if let Some(file) = code {
                storage
                    .write(&format!("plugins/{id}/{file}"), b"\x7fELF not a library")
                    .unwrap();
            }
        }
        storage
    }

    #[test]
    fn code_plugins_are_refused_cleanly_and_data_plugins_load() {
        let kernel = policy(
            Kernel::builder(device())
                .storage(card())
                .plugin_dir("plugins"),
        )
        .build()
        .unwrap();
        let plugins = kernel.plugins();
        for id in ["seven", "six", "x4"] {
            let info = plugins.get(id).unwrap_or_else(|| panic!("{id} is listed"));
            assert_eq!(info.kind, PluginKind::Code, "{id}");
            assert_eq!(info.state, PluginState::Failed, "{id}");
            assert_eq!(info.problem, Some(PluginProblem::Unsupported), "{id}");
            assert!(
                info.enabled,
                "{id}: stays switched on for a build that fits"
            );
        }
        let data = plugins.get("data").unwrap();
        assert_eq!(data.kind, PluginKind::Data);
        assert_eq!(data.problem, None);
        assert_ne!(data.state, PluginState::Failed);
    }

    /// Every Rust source of the shell (read at test time, so a new file is
    /// covered) installs no code loader and links no dynamic loader. This
    /// file is left out: it names what it forbids.
    #[test]
    fn no_code_loader_is_installed_anywhere_in_the_shell() {
        fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&root, &mut files);
        assert!(
            files.len() >= 5,
            "the shell's sources were not found: {files:?}"
        );
        for file in files {
            if file.file_name().is_some_and(|name| name == "code.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&file).unwrap();
            for banned in [".code_plugins(", "CodeLoader", "dlopen", "libloading"] {
                assert!(
                    !source.contains(banned),
                    "{} mentions {banned}",
                    file.display()
                );
            }
        }
        let manifest = include_str!("../Cargo.toml");
        for banned in ["libloading", "dlopen"] {
            assert!(!manifest.contains(banned), "Cargo.toml names {banned}");
        }
    }
}
