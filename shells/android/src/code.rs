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
    allow(dead_code, reason = "the Android app calls it; on a computer only the tests do")
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
        let kernel = policy(Kernel::builder(device()).storage(card()).plugin_dir("plugins"))
            .build()
            .unwrap();
        let plugins = kernel.plugins();
        for id in ["seven", "six", "x4"] {
            let info = plugins.get(id).unwrap_or_else(|| panic!("{id} is listed"));
            assert_eq!(info.kind, PluginKind::Code, "{id}");
            assert_eq!(info.state, PluginState::Failed, "{id}");
            assert_eq!(info.problem, Some(PluginProblem::Unsupported), "{id}");
            assert!(info.enabled, "{id}: stays switched on for a build that fits");
        }
        let data = plugins.get("data").unwrap();
        assert_eq!(data.kind, PluginKind::Data);
        assert_eq!(data.problem, None);
        assert_ne!(data.state, PluginState::Failed);
    }

    /// The shell source installs no code loader and links no dynamic loader,
    /// apart from the policy's own documentation of it.
    #[test]
    fn no_code_loader_is_installed_anywhere_in_the_shell() {
        let sources = [
            ("app.rs", include_str!("app.rs")),
            ("plugin.rs", include_str!("plugin.rs")),
            ("lib.rs", include_str!("lib.rs")),
        ];
        for (name, source) in sources {
            for banned in [".code_plugins(", "CodeLoader", "dlopen", "libloading"] {
                assert!(!source.contains(banned), "{name} mentions {banned}");
            }
        }
        let manifest = include_str!("../Cargo.toml");
        for banned in ["libloading", "dlopen"] {
            assert!(!manifest.contains(banned), "Cargo.toml names {banned}");
        }
    }
}
