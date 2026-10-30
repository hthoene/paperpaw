//! Code plugins (ADR 0024): folders on the storage like data plugins'
//! (`plugin.ini`, translations, settings) with native code beside them
//! (`code.ppn` on the X4 and the X4 Pro). The shell loads the code
//! ([`CodeLoader`], given with
//! [`KernelBuilder::code_plugins`](crate::KernelBuilder::code_plugins));
//! the kernel decides which, at the start only, as the configuration
//! says, and registers each with its folder's data. Switching one takes a
//! restart.
//!
//! Safe mode ([`KernelBuilder::safe_mode`](crate::KernelBuilder::safe_mode))
//! starts without code plugins and switches off the one the last crash was
//! in, or, when that is not known, every one that was on: each stays off
//! until it is switched on again.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;
use paperpaw_sdk::native::{Native, SafeMode, SafeModeReason};
use paperpaw_sdk::{
    Id, Manifest, Plugin, PluginKind, PluginProblem, PluginState, RegisterError, Registrar,
};

use crate::data_plugin::DataPlugin;
use crate::{Kernel, WarningSource};

/// The code plugins fetched for a firmware update that fit this one
/// ([`native::fetched_folder`](paperpaw_sdk::native::fetched_folder)):
/// their folders, to install before the plugins are found, their code
/// named as installed. Those built for another firmware wait (the update
/// may come yet, or went back); a folder whose code the update plugin did
/// not fetch ([`native::remember_fetched`](paperpaw_sdk::native::remember_fetched))
/// is not taken.
pub(crate) fn fetched(storage: &dyn Storage, native: &Native) -> Vec<(Id, String)> {
    paperpaw_sdk::native::fetched_for(storage, native)
        .into_iter()
        .filter_map(|(id, sha256)| {
            let id = Id::parse(&id).ok()?;
            // Once it is named as installed, the code before no longer
            // runs.
            paperpaw_sdk::native::remember_code(storage, id.as_str(), &sha256).then(|| {
                (
                    id.clone(),
                    paperpaw_sdk::native::fetched_folder(id.as_str()),
                )
            })
        })
        .collect()
}

/// Whether a start with code plugins would load any that fit `native`:
/// one installed (a folder of [`PLUGIN_DIR`](paperpaw_sdk::builtin::PLUGIN_DIR)
/// with its code) or fetched for a firmware update. A start without them
/// (an update on trial) asks it once the update is kept: if so, the shell
/// starts again for them.
pub fn code_to_load(storage: &dyn Storage, native: &Native) -> bool {
    if !paperpaw_sdk::native::fetched_for(storage, native).is_empty() {
        return true;
    }
    let mut folders: Vec<String> = Vec::new();
    let _ = storage.list(paperpaw_sdk::builtin::PLUGIN_DIR, &mut |entry| {
        if entry.is_dir && !entry.name.starts_with('.') {
            folders.push(format!(
                "{}/{}",
                paperpaw_sdk::builtin::PLUGIN_DIR,
                entry.name
            ));
        }
    });
    folders.iter().any(|folder| {
        paperpaw_sdk::native::read_header(storage, &native.path(folder))
            .is_some_and(|header| native.fits(&header))
    })
}

/// What loads the code of code plugins: the shell's, which knows where it
/// can run.
pub trait CodeLoader {
    /// Loads the code of the plugin `code.id` and makes its plugin, or
    /// says why it cannot. Called at the start, once for every code plugin
    /// switched on, in the order of their ids. Only the file installed may
    /// run: one whose header's SHA-256 is not `code.sha256` (changed since
    /// it was installed) is [`PluginProblem::Changed`], and its content
    /// must hold to its header.
    fn load(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem>;

    /// Every code plugin of this start is loaded (in safe mode, none
    /// was): e.g. say how much room is left for them
    /// ([`paperpaw_sdk::native::keep_code_room`]).
    fn finish(&mut self, _storage: &dyn Storage) {}
}

/// A code plugin's file to load ([`CodeLoader::load`]).
#[derive(Debug, Clone, Copy)]
pub struct CodeFile<'a> {
    /// The plugin's id, as its `plugin.ini` says: what a crash in its
    /// code is told by.
    pub id: &'a Id,
    /// The file: its folder's [`Native::file`].
    pub path: &'a str,
    /// The SHA-256 its header carried when it was installed
    /// ([`paperpaw_sdk::native::installed_code`]).
    pub sha256: [u8; 32],
}

/// How the kernel loads code plugins at this start.
pub(crate) struct CodePlugins {
    pub(crate) native: Native,
    pub(crate) loader: Box<dyn CodeLoader>,
    /// This start goes without them.
    pub(crate) safe_mode: Option<SafeMode>,
}

/// A code plugin: its code, then its folder's data.
struct WithData {
    code: Box<dyn Plugin>,
    data: DataPlugin,
}

impl Plugin for WithData {
    /// The code's, at the version its `plugin.ini` names: the one the
    /// store, the installer and updates go by.
    fn manifest(&self) -> Manifest {
        let mut manifest = self.code.manifest();
        manifest.version = self.data.manifest().version;
        manifest
    }

    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        self.code.register(registrar)?;
        self.data.register(registrar)
    }
}

impl Kernel {
    /// Registers the code plugins found among the storage's plugins, in
    /// the order of their ids, as the configuration (`disabled`) and safe
    /// mode say.
    pub(crate) fn add_code_plugins(
        &mut self,
        mut found: Vec<DataPlugin>,
        disabled: &[String],
        code: Option<CodePlugins>,
    ) {
        found.sort_by_cached_key(|plugin| plugin.manifest().id);
        let Some(CodePlugins {
            mut native,
            mut loader,
            safe_mode,
        }) = code
        else {
            for plugin in found {
                self.list_code_plugin(
                    &plugin,
                    disabled,
                    PluginState::Failed,
                    PluginProblem::Unsupported,
                );
            }
            return;
        };
        let storage = self.shared.storage.clone();
        if let Some(mut safe_mode) = safe_mode {
            self.hold_back(found, disabled, &mut safe_mode);
            native.safe_mode = Some(safe_mode);
            self.shared.native = Some(native);
            loader.finish(storage.as_ref());
            return;
        }
        // Discovery settled pending install marks before this snapshot.
        // A failed read grants no code permission, as an individual lookup did.
        let installed = paperpaw_sdk::native::InstalledCodes::load(
            storage.as_ref(),
            found
                .iter()
                .map(|plugin| plugin.manifest().id)
                .filter(|id| !disabled.iter().any(|off| off == id.as_str())),
        )
        .unwrap_or_default();
        for plugin in found {
            let manifest = plugin.manifest();
            let id = manifest.id.clone();
            if disabled.iter().any(|d| d == id.as_str()) {
                self.register_off(&plugin, manifest);
                self.set_code_info(&id, None);
                continue;
            }
            let path = native.path(plugin.folder());
            // Only code installed runs: not a file put there otherwise.
            let Some(sha256) = installed.get(id.as_str()) else {
                let message = format!("{path}: never installed; not loaded");
                self.warn(WarningSource::Plugin(id.clone()), None, message);
                let problem = PluginProblem::NotInstalled;
                self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
                continue;
            };
            let file = CodeFile {
                id: &id,
                path: &path,
                sha256,
            };
            let loaded = loader.load(storage.as_ref(), &file).and_then(|code| {
                if code.manifest().id == id {
                    Ok(code)
                } else {
                    Err(PluginProblem::Mismatch)
                }
            });
            match loaded {
                Ok(code) => {
                    // Its `plugin.ini` names its version (0.1.0 where it
                    // names none); code saying another is the author's slip
                    // (`paperpaw-dev package` finds it), told, not refused.
                    let (declared, built) = (plugin.manifest().version, code.manifest().version);
                    if built != declared {
                        let message = format!(
                            "{path}: its code says version {built}, its plugin.ini {declared}: \
                             taken as {declared}"
                        );
                        self.warn(WarningSource::Plugin(id.clone()), None, message);
                    }
                    let plugin = WithData { code, data: plugin };
                    let manifest = plugin.manifest();
                    self.register_plugin(&plugin, manifest);
                    self.set_code_info(&id, None);
                }
                Err(problem) => {
                    let message = format!("{path}: not loaded ({problem:?})");
                    self.warn(WarningSource::Plugin(id.clone()), None, message);
                    self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
                }
            }
        }
        drop(installed);
        loader.finish(storage.as_ref());
        self.shared.native = Some(native);
    }

    /// A code plugin installed while PaperPaw runs (`Command::LoadPlugin`):
    /// listed, its code loaded at the next start.
    pub(crate) fn add_code_plugin_later(&mut self, plugin: &DataPlugin, off: bool) {
        let disabled: Vec<String> = if off {
            alloc::vec![String::from(plugin.manifest().id.as_str())]
        } else {
            Vec::new()
        };
        let problem = match &self.shared.native {
            Some(_) => None,
            None => Some(PluginProblem::Unsupported),
        };
        let state = if problem.is_some() {
            PluginState::Failed
        } else {
            PluginState::Off
        };
        self.list_code_plugin(plugin, &disabled, state, PluginProblem::Unsupported);
        self.set_code_info(&plugin.manifest().id, problem);
    }

    /// Safe mode: no code is loaded; the plugin the crash was in is
    /// switched off, or, when that is not known, every one that was on.
    fn hold_back(&mut self, found: Vec<DataPlugin>, disabled: &[String], safe_mode: &mut SafeMode) {
        let culprit = match safe_mode.reason {
            SafeModeReason::FailedStarts => safe_mode.crashed.clone(),
            _ => None,
        };
        for plugin in found {
            let id = plugin.manifest().id;
            let on = !disabled.iter().any(|d| d == id.as_str());
            let switch_off = on && culprit.as_ref().is_none_or(|c| c == id.as_str());
            if switch_off {
                self.save_config("plugins", id.as_str(), "off");
                safe_mode.switched_off.push(id.clone());
            }
            let problem = if culprit.is_some() && switch_off {
                PluginProblem::Crashed
            } else {
                PluginProblem::SafeMode
            };
            let disabled: Vec<String> = if switch_off {
                alloc::vec![String::from(id.as_str())]
            } else {
                disabled.to_vec()
            };
            self.list_code_plugin(&plugin, &disabled, PluginState::Off, problem);
        }
    }

    /// Lists a code plugin whose code is not loaded, in `state` (when
    /// switched on) and why: its translations only, as for one switched
    /// off.
    fn list_code_plugin(
        &mut self,
        plugin: &DataPlugin,
        disabled: &[String],
        state: PluginState,
        problem: PluginProblem,
    ) {
        let manifest = plugin.manifest();
        let id = manifest.id.clone();
        self.register_off(plugin, manifest);
        let on = !disabled.iter().any(|d| d == id.as_str());
        if let Some(info) = self.shared.plugins.get_mut(id.as_str()) {
            info.enabled = on;
            if on {
                info.state = state;
            }
        }
        self.set_code_info(&id, Some(problem));
    }

    fn set_code_info(&mut self, id: &Id, problem: Option<PluginProblem>) {
        if let Some(info) = self.shared.plugins.get_mut(id.as_str()) {
            info.kind = PluginKind::Code;
            info.problem = problem;
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::borrow::Cow;
    use alloc::rc::Rc;
    use core::cell::RefCell;

    use paperpaw_sdk::{ActionSpec, MemoryStorage, Version};

    use super::*;
    use crate::tests::DEVICE;

    /// A plugin whose code the loader "loaded": one action.
    struct Code(&'static str);

    impl Plugin for Code {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static(self.0), "Code", Version::new(1, 0, 0))
                .description("Native code.")
                .uses(paperpaw_sdk::Uses::NOTHING)
        }

        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            let id =
                Id::parse(&format!("{}.hello", self.0)).map_err(|_| RegisterError::Invalid {
                    message: Cow::Borrowed("id"),
                })?;
            r.add_action(ActionSpec::new(id, "hello"))
        }
    }

    /// Loads from a table, noting what it was asked.
    #[derive(Clone, Default)]
    struct Loader {
        asked: Rc<RefCell<Vec<String>>>,
        /// The ids it was told, with the paths asked.
        ids: Rc<RefCell<Vec<String>>>,
        finished: Rc<RefCell<bool>>,
        answers: Vec<(&'static str, Result<&'static str, PluginProblem>)>,
    }

    impl CodeLoader for Loader {
        fn load(
            &mut self,
            _: &dyn Storage,
            code: &CodeFile<'_>,
        ) -> Result<Box<dyn Plugin>, PluginProblem> {
            let path = code.path;
            self.asked.borrow_mut().push(String::from(path));
            self.ids.borrow_mut().push(String::from(code.id.as_str()));
            if code.sha256 != installed(code.id.as_str()) {
                return Err(PluginProblem::Changed);
            }
            let answer = self
                .answers
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, a)| *a);
            match answer {
                Some(Ok(id)) => Ok(Box::new(Code(id))),
                Some(Err(problem)) => Err(problem),
                None => Err(PluginProblem::Damaged),
            }
        }

        fn finish(&mut self, _: &dyn Storage) {
            *self.finished.borrow_mut() = true;
        }
    }

    /// The digest a test plugin's code was installed with.
    fn installed(id: &str) -> [u8; 32] {
        [id.len() as u8; 32]
    }

    /// The storage's code plugins named as installed.
    fn named_installed(storage: MemoryStorage, ids: &[&str]) -> MemoryStorage {
        for id in ids {
            assert!(paperpaw_sdk::native::remember_code(
                &storage,
                id,
                &installed(id)
            ));
        }
        storage
    }

    fn storage() -> MemoryStorage {
        let storage = MemoryStorage::new()
            .with(
                "plugins/zeta/plugin.ini",
                "[plugin]\nname = Zeta\nversion = 1.0.0\n",
            )
            .with("plugins/zeta/code.ppn", "PPNP")
            .with(
                "plugins/hello/plugin.ini",
                "[plugin]\nname = Hello\nversion = 1.0.0\n",
            )
            .with("plugins/hello/code.ppn", "PPNP")
            .with(
                "plugins/hello/i18n/en.ini",
                "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nhello.greeting = Hi\n",
            )
            .with("plugins/theme/plugin.ini", "[plugin]\nname = Theme\n");
        named_installed(storage, &["hello", "zeta"])
    }

    fn loader() -> Loader {
        Loader {
            answers: alloc::vec![
                ("plugins/hello/code.ppn", Ok("hello")),
                ("plugins/zeta/code.ppn", Ok("zeta")),
            ],
            ..Loader::default()
        }
    }

    fn kernel(loader: &Loader, config: &str, safe_mode: Option<SafeMode>) -> Kernel {
        let builder = Kernel::builder(DEVICE)
            .storage(storage())
            .user_config(config)
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()));
        match safe_mode {
            Some(safe_mode) => builder.safe_mode(safe_mode),
            None => builder,
        }
        .build()
        .unwrap()
    }

    fn info<'a>(kernel: &'a Kernel, id: &str) -> &'a paperpaw_sdk::PluginInfo {
        kernel.plugins().get(id).unwrap()
    }

    #[test]
    fn code_is_loaded_in_the_order_of_ids_with_its_folder() {
        let loader = loader();
        let kernel = kernel(&loader, "", None);
        assert_eq!(
            *loader.asked.borrow(),
            ["plugins/hello/code.ppn", "plugins/zeta/code.ppn"]
        );
        assert!(*loader.finished.borrow());
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.kind, hello.problem),
            (PluginState::Running, PluginKind::Code, None)
        );
        assert_eq!(info(&kernel, "theme").kind, PluginKind::Data);
        assert!(kernel.shared.actions.contains("hello.hello"), "its code's");
        assert_eq!(
            kernel.translator().get("hello.greeting"),
            Some("Hi"),
            "its folder's"
        );
        let native = kernel.shared.native.as_ref().unwrap();
        assert_eq!(native.safe_mode, None);
    }

    #[test]
    fn switched_off_its_code_is_not_loaded() {
        let loader = loader();
        let kernel = kernel(&loader, "[plugins]\nhello = off\n", None);
        assert_eq!(*loader.asked.borrow(), ["plugins/zeta/code.ppn"]);
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.enabled, hello.kind),
            (PluginState::Off, false, PluginKind::Code)
        );
    }

    #[test]
    fn code_that_does_not_fit_is_listed_with_why() {
        let loader = Loader {
            answers: alloc::vec![
                ("plugins/hello/code.ppn", Err(PluginProblem::OtherBuild)),
                ("plugins/zeta/code.ppn", Ok("other")),
            ],
            ..Loader::default()
        };
        let kernel = kernel(&loader, "", None);
        let hello = info(&kernel, "hello");
        assert_eq!(hello.state, PluginState::Failed);
        assert!(hello.enabled, "it stays switched on, for a build that fits");
        assert_eq!(hello.problem, Some(PluginProblem::OtherBuild));
        assert_eq!(info(&kernel, "zeta").problem, Some(PluginProblem::Mismatch));
        assert!(!kernel.shared.actions.contains("other.hello"));

        // Where PaperPaw loads no code.
        let kernel = Kernel::builder(DEVICE).storage(storage()).build().unwrap();
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.problem),
            (PluginState::Failed, Some(PluginProblem::Unsupported))
        );
        assert!(kernel.shared.native.is_none());
    }

    /// The version its `plugin.ini` names is the plugin's (the store,
    /// the installer and updates go by it): code that says another runs
    /// at that version, and it is told; nothing is refused for it.
    #[test]
    fn code_of_another_version_than_its_folder_says_runs_at_the_folders() {
        let loader = loader();
        let storage = storage()
            .with(
                "plugins/hello/plugin.ini",
                "[plugin]\nname = Hello\nversion = 2.0.0\n",
            )
            .with("plugins/zeta/plugin.ini", "[plugin]\nname = Zeta\n");
        let mut kernel = kernel_on(storage, &loader, "");
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.problem, hello.manifest.version),
            (PluginState::Running, None, Version::new(2, 0, 0))
        );
        assert!(kernel.shared.actions.contains("hello.hello"));
        // Where it names none, 0.1.0, as for any plugin.
        assert_eq!(
            info(&kernel, "zeta").manifest.version,
            Version::new(0, 1, 0)
        );
        let warnings = kernel.take_warnings();
        assert!(
            warnings.iter().any(|w| w
                .message
                .contains("its code says version 1.0.0, its plugin.ini 2.0.0")),
            "{warnings:?}"
        );
    }

    #[test]
    fn only_code_installed_runs() {
        let loader = loader();
        let storage = storage()
            .with("plugins/copied/plugin.ini", "[plugin]\nname = Copied\n")
            .with("plugins/copied/code.ppn", "PPNP");
        // Hello's file is not the one installed.
        assert!(paperpaw_sdk::native::remember_code(
            &storage, "hello", &[9; 32]
        ));
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
            .build()
            .unwrap();
        // Never installed: its code is not even read.
        assert!(!loader.asked.borrow().iter().any(|p| p.contains("copied")));
        let copied = info(&kernel, "copied");
        assert_eq!(
            (copied.state, copied.problem),
            (PluginState::Failed, Some(PluginProblem::NotInstalled))
        );
        assert!(copied.enabled, "it runs once installed");
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.problem),
            (PluginState::Failed, Some(PluginProblem::Changed))
        );
        assert_eq!(info(&kernel, "zeta").state, PluginState::Running);
    }

    #[test]
    fn code_is_loaded_by_its_plugins_id_in_any_folder() {
        let loader = Loader {
            answers: alloc::vec![("plugins/my-clock/code.ppn", Ok("clock"))],
            ..Loader::default()
        };
        let storage = MemoryStorage::new()
            .with(
                "plugins/my-clock/plugin.ini",
                "[plugin]\nid = clock\nname = Clock\n",
            )
            .with("plugins/my-clock/code.ppn", "PPNP");
        let storage = named_installed(storage, &["clock"]);
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
            .build()
            .unwrap();
        // Told by its id, what safe mode and a crash name it by.
        assert_eq!(*loader.ids.borrow(), ["clock"]);
        assert_eq!(*loader.asked.borrow(), ["plugins/my-clock/code.ppn"]);
        assert_eq!(info(&kernel, "clock").state, PluginState::Running);
    }

    #[test]
    fn safe_mode_switches_off_the_plugin_the_crash_was_in() {
        let loader = loader();
        let mut safe_mode = SafeMode::new(SafeModeReason::FailedStarts);
        safe_mode.crashed = Some(String::from("zeta"));
        let kernel = kernel(&loader, "", Some(safe_mode));
        assert!(loader.asked.borrow().is_empty(), "no code at all");
        // Told that loading is over: it says how much room is left.
        assert!(*loader.finished.borrow());
        let zeta = info(&kernel, "zeta");
        assert_eq!(
            (zeta.enabled, zeta.problem),
            (false, Some(PluginProblem::Crashed))
        );
        // Not loaded at this start, but on for the next.
        let hello = info(&kernel, "hello");
        assert_eq!((hello.state, hello.enabled), (PluginState::Off, true));
        assert_eq!(hello.problem, Some(PluginProblem::SafeMode));
        assert!(hello.restart_needed());
        let config = paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 4096).unwrap();
        let config = String::from_utf8(config).unwrap();
        assert!(config.contains("zeta = off"), "{config}");
        assert!(!config.contains("hello"), "{config}");
        let native = kernel.shared.native.as_ref().unwrap();
        let safe = native.safe_mode.as_ref().unwrap();
        assert_eq!(safe.switched_off, [Id::new_static("zeta")]);
    }

    #[test]
    fn safe_mode_without_a_culprit_switches_every_code_plugin_off() {
        for reason in [SafeModeReason::FailedStarts, SafeModeReason::KeyHeld] {
            let loader = loader();
            let mut safe_mode = SafeMode::new(reason);
            // Held at power-on, a crash before is not what it is about.
            safe_mode.crashed = (reason == SafeModeReason::KeyHeld).then(|| String::from("zeta"));
            let kernel = kernel(&loader, "[plugins]\nzeta = off\n", Some(safe_mode));
            let native = kernel.shared.native.as_ref().unwrap();
            let safe = native.safe_mode.as_ref().unwrap();
            assert_eq!(
                safe.switched_off,
                [Id::new_static("hello")],
                "{reason:?}: zeta was off"
            );
            let hello = info(&kernel, "hello");
            assert_eq!(
                (hello.enabled, hello.problem),
                (false, Some(PluginProblem::SafeMode))
            );
        }
    }

    #[test]
    fn code_installed_while_running_starts_after_a_restart() {
        let loader = loader();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
            .build()
            .unwrap();
        let _ = kernel.storage().create_dir("plugins/hello");
        for (path, data) in [
            ("plugins/hello/plugin.ini", "[plugin]\nname = Hello\n"),
            ("plugins/hello/code.ppn", "PPNP"),
        ] {
            kernel.storage().write(path, data.as_bytes()).unwrap();
        }
        kernel.load_plugin("plugins/hello");
        assert!(loader.asked.borrow().is_empty());
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.enabled, hello.kind),
            (PluginState::Off, true, PluginKind::Code)
        );
        assert!(hello.restart_needed());
    }

    #[test]
    fn plugins_the_shell_put_in_folders_install_before_the_start() {
        let loader = loader();
        let storage = MemoryStorage::new()
            .with(
                "plugins/hello/plugin.ini",
                "[plugin]\nname = Hello\nversion = 0.1.0\n",
            )
            .with(
                "plugins/.hello.part/plugin.ini",
                "[plugin]\nname = Hello\nversion = 1.0.0\n",
            )
            .with("plugins/.hello.part/code.ppn", "PPNP")
            .with("plugins/.other.part/plugin.ini", "[plugin]\nid = zeta\n");
        // As the shell's installer names it.
        let storage = named_installed(storage, &["hello"]);
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
            .install_plugin(Id::new_static("hello"), "plugins/.hello.part")
            .install_plugin(Id::new_static("other"), "plugins/.other.part")
            .build()
            .unwrap();
        // In place of the version before, its code loaded at this start.
        assert_eq!(*loader.asked.borrow(), ["plugins/hello/code.ppn"]);
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.state, hello.kind),
            (PluginState::Running, PluginKind::Code)
        );
        let storage = kernel.storage();
        assert!(!storage.exists("plugins/.hello.part/plugin.ini"));
        let ini = paperpaw_sdk::read_file(storage, "plugins/hello/plugin.ini", 512).unwrap();
        assert!(String::from_utf8(ini).unwrap().contains("1.0.0"));
        // Not what it says it is: refused, its folder gone.
        assert!(!storage.exists("plugins/.other.part/plugin.ini"));
        assert!(kernel.plugins().get("other").is_none());
        let warnings = kernel.take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("not the plugin `other`")),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_code_plugin_updated_while_it_runs_takes_over_at_the_restart() {
        let loader = loader();
        let mut kernel = kernel(&loader, "", None);
        assert_eq!(info(&kernel, "hello").state, PluginState::Running);
        for (path, data) in [
            (
                "plugins/.hello.part/plugin.ini",
                "[plugin]
id = hello
name = Hello
version = 2.0.0
",
            ),
            ("plugins/.hello.part/code.ppn", "PPNP"),
        ] {
            kernel.storage().write(path, data.as_bytes()).unwrap();
        }
        kernel.install_plugin(&Id::new_static("hello"), "plugins/.hello.part");
        assert!(kernel.take_warnings().is_empty());
        // The code before runs on, listed with the update it waits for.
        let hello = info(&kernel, "hello");
        assert_eq!((hello.state, hello.enabled), (PluginState::Running, true));
        assert_eq!(hello.update, Some(Version::new(2, 0, 0)));
        assert!(hello.restart_needed());
        let storage = kernel.storage();
        assert!(storage.exists("plugins/hello/plugin.ini"));
        assert!(!storage.exists("plugins/.hello.part/code.ppn"));
        assert!(!storage.exists("plugins/.hello.old/code.ppn"));
    }

    #[test]
    fn pending_install_marks_settle_before_the_startup_snapshot() {
        use paperpaw_sdk::install::marks::{self, From, Marks};
        let storage = storage();
        let native = Native::new("code.ppn");
        assert!(paperpaw_sdk::native::remember_code(
            &storage, "hello", &[9; 32]
        ));
        storage
            .write(
                "plugins/hello/code.ppn",
                &code_file(native.fingerprint.0, installed("hello")),
            )
            .unwrap();
        marks::write(
            &storage,
            "plugins/hello",
            &Marks::new(From::Store(Version::new(1, 0, 0))).code(installed("hello")),
        )
        .unwrap();
        let loader = loader();
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .code_plugins(native, Box::new(loader.clone()))
            .build()
            .unwrap();
        assert_eq!(info(&kernel, "hello").state, PluginState::Running);
        assert_eq!(loader.asked.borrow().len(), 2);
        assert!(!kernel.storage().exists("plugins/hello/.install-marks"));
    }

    /// The file of a code plugin with `fingerprint`, whose header's
    /// SHA-256 is `sha256` (the test loader checks only that).
    fn code_file(fingerprint: u64, sha256: [u8; 32]) -> alloc::vec::Vec<u8> {
        let header = paperpaw_sdk::native::Header {
            chip: paperpaw_sdk::native::Chip::from_id(5).unwrap(),
            fingerprint,
            base: 0x3c00_0000,
            image_len: 128,
            code_at: 96,
            entry: 0,
            api: 32,
            relocs: 0,
            align: 16,
            compiler: 1,
            hot_at: 0,
            hot_len: 0,
            patches: 0,
            needs_api: 0,
            families: [0; 8],
            interface: crate::sdk::native::INTERFACE,
            sha256,
        };
        let mut file = header.to_bytes().to_vec();
        file.extend_from_slice(&[0; 128]);
        file
    }

    /// A storage with `stats` fetched for a firmware update: its folder,
    /// its code built for `fingerprint`, named as fetched if `named`.
    fn fetched(fingerprint: u64, named: bool) -> MemoryStorage {
        let storage = storage()
            .with(
                "plugins/.stats.next/plugin.ini",
                "[plugin]\nid = stats\nname = Statistics\nversion = 1.0.0\n",
            )
            .with(".paperpaw/plugins/stats/statistics.sqlite3", "kept");
        storage
            .write(
                "plugins/.stats.next/code.ppn",
                &code_file(fingerprint, installed("stats")),
            )
            .unwrap();
        if named {
            assert!(paperpaw_sdk::native::remember_fetched(
                &storage,
                "stats",
                &installed("stats")
            ));
        }
        storage
    }

    fn kernel_on(storage: MemoryStorage, loader: &Loader, config: &str) -> Kernel {
        Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(config)
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
            .build()
            .unwrap()
    }

    fn stats_loader() -> Loader {
        let mut loader = loader();
        loader.answers.push(("plugins/stats/code.ppn", Ok("stats")));
        loader
    }

    #[test]
    fn code_fetched_for_this_firmware_is_installed_at_its_start() {
        let fingerprint = Native::new("code.ppn").fingerprint.0;
        let storage = fetched(fingerprint, true);
        let loader = stats_loader();
        let kernel = kernel_on(storage, &loader, "");
        let storage = kernel.storage();
        assert_eq!(info(&kernel, "stats").state, PluginState::Running);
        assert_eq!(info(&kernel, "stats").kind, PluginKind::Code);
        assert!(
            loader
                .asked
                .borrow()
                .iter()
                .any(|p| p == "plugins/stats/code.ppn")
        );
        assert!(!storage.exists("plugins/.stats.next/code.ppn"));
        // Its data and settings belong to its id: they stay.
        assert!(storage.exists(".paperpaw/plugins/stats/statistics.sqlite3"));
    }

    #[test]
    fn code_fetched_for_another_firmware_waits() {
        let fingerprint = Native::new("code.ppn").fingerprint.0 ^ 1;
        let storage = fetched(fingerprint, true);
        let loader = stats_loader();
        let kernel = kernel_on(storage, &loader, "");
        let storage = kernel.storage();
        assert!(kernel.plugins().get("stats").is_none());
        assert!(storage.exists("plugins/.stats.next/code.ppn"));
    }

    #[test]
    fn a_folder_the_update_did_not_fetch_is_not_taken() {
        // Put there otherwise (e.g. over the Wi-Fi transfer).
        let fingerprint = Native::new("code.ppn").fingerprint.0;
        let storage = fetched(fingerprint, false);
        let loader = stats_loader();
        let kernel = kernel_on(storage, &loader, "");
        assert!(kernel.plugins().get("stats").is_none());
        let storage = kernel.storage();
        assert!(paperpaw_sdk::native::installed_code(storage, "stats").is_none());
    }

    #[test]
    fn safe_mode_leaves_code_fetched_where_it_is() {
        let fingerprint = Native::new("code.ppn").fingerprint.0;
        let storage = fetched(fingerprint, true);
        let loader = stats_loader();
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
            .safe_mode(SafeMode::new(SafeModeReason::KeyHeld))
            .build()
            .unwrap();
        assert!(kernel.storage().exists("plugins/.stats.next/code.ppn"));
    }

    #[test]
    fn a_start_without_code_plugins_leaves_code_fetched_for_the_next() {
        // An update's trial start loads no code plugins: what was fetched
        // for it waits, and the firmware, seeing it, starts once more
        // when the update is kept.
        let native = Native::new("code.ppn");
        let storage = fetched(native.fingerprint.0, true);
        let kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
        let storage = kernel.storage();
        assert!(storage.exists("plugins/.stats.next/code.ppn"));
        let waiting = paperpaw_sdk::native::fetched_for(storage, &native);
        assert_eq!(waiting, [("stats".into(), installed("stats"))]);
        // For another firmware, nothing waits.
        let mut other = Native::new("code.ppn");
        other.fingerprint.0 ^= 1;
        assert!(paperpaw_sdk::native::fetched_for(storage, &other).is_empty());
        assert!(super::code_to_load(storage, &native));
        assert!(!super::code_to_load(storage, &other));
    }

    /// An update that keeps the fingerprint fetches nothing: the code
    /// installed before still fits, and a start once the update is kept
    /// loads it (the trial's start went without).
    #[test]
    fn code_installed_that_fits_is_code_to_load() {
        let native = Native::new("code.ppn");
        let plugins = storage().with(
            "plugins/stats/plugin.ini",
            "[plugin]\nid = stats\nname = Statistics\nversion = 1.0.0\n",
        );
        assert!(!super::code_to_load(&plugins, &native));
        plugins
            .write(
                "plugins/stats/code.ppn",
                &code_file(native.fingerprint.0, installed("stats")),
            )
            .unwrap();
        assert!(super::code_to_load(&plugins, &native));
        let mut other = Native::new("code.ppn");
        other.fingerprint.0 ^= 1;
        assert!(!super::code_to_load(&plugins, &other));
        // A folder being installed or put aside is not one to load.
        let aside = storage().with("plugins/.stats.old/code.ppn", "");
        aside
            .write(
                "plugins/.stats.old/code.ppn",
                &code_file(native.fingerprint.0, installed("stats")),
            )
            .unwrap();
        assert!(!super::code_to_load(&aside, &native));
    }
}

#[cfg(test)]
#[path = "code_plugin_startup_tests.rs"]
mod startup_tests;
