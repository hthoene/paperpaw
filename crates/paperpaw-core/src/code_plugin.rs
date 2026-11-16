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

use paperpaw_native::header5;
use paperpaw_platform::Storage;
use paperpaw_sdk::native::{Native, SafeMode, SafeModeReason};
use paperpaw_sdk::{Id, PluginKind, PluginProblem, PluginState};

use crate::data_plugin::DataPlugin;
use crate::{Kernel, WarningSource};

/// The SHA-256 in the header of the code file at `path`, if this firmware
/// loads the file: a format-5 file (interface 7) its loader reads (`loader`,
/// the chip, interface and minor it takes; `None` where it reads none). A
/// file of the format before is never loaded, whatever it says it fits.
fn code_that_fits(
    storage: &dyn Storage,
    path: &str,
    loader: Option<&header5::Host>,
) -> Option<[u8; 32]> {
    let mut head = [0u8; header5::HEADER_LEN];
    let mut at = 0;
    while at < head.len() {
        match storage.read_at(path, at as u64, &mut head[at..]) {
            Ok(0) | Err(_) => return None,
            Ok(n) => at += n,
        }
    }
    let len = storage.size(path).ok()?;
    let header = header5::Header5::read(&head, len, loader?).ok()?;
    Some(header.sha256)
}

/// The plugins fetched for a firmware update that wait for this one: their
/// folders ([`native::fetched_folder`](paperpaw_sdk::native::fetched_folder))
/// hold code this firmware loads ([`code_that_fits`]) and fetched by the
/// update plugin ([`native::remember_fetched`](paperpaw_sdk::native::remember_fetched)),
/// not put there otherwise. Their ids and the SHA-256 of their headers, sorted.
pub(crate) fn fetched_for(
    storage: &dyn Storage,
    native: &Native,
    loader: Option<&header5::Host>,
) -> Vec<(String, [u8; 32])> {
    let mut ids: Vec<String> = Vec::new();
    let _ = storage.list(paperpaw_sdk::builtin::PLUGIN_DIR, &mut |entry| {
        if entry.is_dir
            && let Some(id) = paperpaw_sdk::native::fetched_plugin(entry.name)
        {
            ids.push(String::from(id));
        }
    });
    paperpaw_sort::sort(&mut ids);
    ids.into_iter()
        .filter_map(|id| {
            let path = native.path(&paperpaw_sdk::native::fetched_folder(&id));
            let sha256 = code_that_fits(storage, &path, loader)?;
            (paperpaw_sdk::native::fetched_code(storage, &id) == Some(sha256))
                .then_some((id, sha256))
        })
        .collect()
}

/// The code plugins fetched for a firmware update that fit this one
/// ([`fetched_for`]): their folders, to install before the plugins are
/// found, their code named as installed. Those built for another firmware
/// wait (the update may come yet, or went back); a folder whose code the
/// update plugin did not fetch is not taken.
pub(crate) fn fetched(
    storage: &dyn Storage,
    native: &Native,
    loader: Option<&header5::Host>,
) -> Vec<(Id, String)> {
    fetched_for(storage, native, loader)
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

/// Whether a start with code plugins would load any this firmware loads
/// (`code_that_fits`: `loader` is what its loader reads of interface-7
/// files, `None` where it reads none): one installed (a folder of
/// [`PLUGIN_DIR`](paperpaw_sdk::builtin::PLUGIN_DIR) with its code) or
/// fetched for a firmware update. A start without them (an update on trial)
/// asks it once the update is kept: if so, the shell starts again for them.
pub fn code_to_load(
    storage: &dyn Storage,
    native: &Native,
    loader: Option<&header5::Host>,
) -> bool {
    if !fetched_for(storage, native, loader).is_empty() {
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
    folders
        .iter()
        .any(|folder| code_that_fits(storage, &native.path(folder), loader).is_some())
}

/// What loads the code of code plugins: the shell's, which knows where it
/// can run.
pub trait CodeLoader {
    /// Hosts the code of the plugin `code.id`, an interface-7 native plugin,
    /// ready to be admitted ([`NativeAdmission`]), or says why it cannot run.
    /// Called at the start, once for every code plugin switched on, in the
    /// order of their ids. Only the file installed may run: one whose
    /// header's SHA-256 is not `code.sha256` (changed since it was
    /// installed) is [`PluginProblem::Changed`], and its content must hold
    /// to its header (also where `code.sha256` is `None`: never installed).
    /// `Ok(None)` is "not a file I run" (not an interface-7
    /// file, or a loader that runs none): the kernel says why from the file
    /// itself (`file_problem`: built for an older PaperPaw, needing a
    /// newer one, or damaged). The code runs nothing of the plugin's before
    /// the kernel admits it: the kernel registers its declarations (screens,
    /// settings, actions, …) with the callbacks it committed, all or none.
    ///
    /// # Errors
    /// Why the code is refused.
    fn load_native(
        &mut self,
        _storage: &dyn Storage,
        _code: &CodeFile<'_>,
    ) -> Result<Option<Box<dyn NativeAdmission>>, PluginProblem> {
        Ok(None)
    }

    /// What this loader reads of interface-7 files: the chip, interface and
    /// minor it takes; `None` (the default) where it runs none. The one
    /// source of it: the kernel installs the code fetched for a firmware
    /// update by it, and the shell asks [`code_to_load`] with the same.
    fn reads(&self) -> Option<header5::Host> {
        None
    }

    /// Called once per start, before the first plugin is asked
    /// ([`load_native`](Self::load_native)); not in safe
    /// mode, where no plugin is loaded. A shell that offers host services to
    /// admission wires them here (`storage`: the card, as an `Rc`), so every
    /// plugin is admitted against what is wired at this start and not what an
    /// earlier build left.
    fn prepare(&mut self, _storage: &alloc::rc::Rc<dyn Storage>) {}

    /// Every code plugin of this start is loaded (in safe mode, none
    /// was): e.g. say how much room is left for them
    /// ([`paperpaw_sdk::native::keep_code_room`]).
    fn finish(&mut self, _storage: &dyn Storage) {}

    /// Where the declared hyphenation tables of the data plugins go, if this
    /// shell serves `text.hyphenate` (`storage`: the card the tables are read
    /// from). Asked once, after [`finish`](Self::finish); from then on every
    /// plugin added or removed and every change of the card reaches the sink
    /// ([`HyphenTables`](crate::native_host::HyphenTables)). A shell makes
    /// its one `HostResources::wire` call here, so it wires its other host
    /// views (`core.ui`) with the tables: a refused wiring leaves them all
    /// unwired, and the shell says so.
    fn hyphenation_sink(
        &mut self,
        _storage: &alloc::rc::Rc<dyn Storage>,
    ) -> Option<crate::native_host::HyphenTables> {
        None
    }
}

/// A hosted native plugin the shell hands the kernel to admit
/// ([`CodeLoader::load_native`]): the shell's own host type stays inside it
/// (`Kernel::register_native_in` is generic over it).
pub trait NativeAdmission {
    /// Admits the plugin with the kernel, once and before the first frame.
    /// `folder` is the plugin's folder on the storage, which the font and
    /// hyphenation files its declarations name are relative to. All or
    /// nothing: on an error nothing of the plugin is registered and the
    /// hosted plugin is destroyed.
    ///
    /// # Errors
    /// Why the plugin was refused, as a code plugin's problem.
    fn admit(self: Box<Self>, kernel: &mut Kernel, folder: &str) -> Result<(), PluginProblem>;

    /// Whether the plugin declared the feature `key` (`uses.install`) and
    /// the shell serves it: asked before [`admit`](Self::admit) of a plugin
    /// that was never installed, which may not use every feature.
    fn declares(&self, key: &str) -> bool;
}

/// A hosted plugin of the tests: admitted with nothing registered.
#[cfg(test)]
pub(crate) struct Admitted;

#[cfg(test)]
impl NativeAdmission for Admitted {
    fn admit(self: Box<Self>, _: &mut Kernel, _: &str) -> Result<(), PluginProblem> {
        Ok(())
    }

    fn declares(&self, _: &str) -> bool {
        false
    }
}

/// A code plugin's file to load ([`CodeLoader::load_native`]).
#[derive(Debug, Clone, Copy)]
pub struct CodeFile<'a> {
    /// The plugin's id, as its `plugin.ini` says: what a crash in its
    /// code is told by.
    pub id: &'a Id,
    /// The file: its folder's [`Native::file`].
    pub path: &'a str,
    /// The SHA-256 its header carried when it was installed
    /// ([`paperpaw_sdk::native::installed_code`]); `None` for a file copied
    /// to the card and never installed, which runs as it lies there
    /// (Decision 205): its content must still hold to its own header.
    pub sha256: Option<[u8; 32]>,
    /// Kernel dependencies retained by the flat plugin adapter.
    pub host: &'a crate::native_host::FlatHost,
}

/// How the kernel loads code plugins at this start.
pub(crate) struct CodePlugins {
    pub(crate) native: Native,
    pub(crate) loader: Box<dyn CodeLoader>,
    /// This start goes without them.
    pub(crate) safe_mode: Option<SafeMode>,
}

/// What a start knows of the code among the card's plugins: how it loads
/// (`None`: this start loads none) and which ids are built-in plugins'
/// ([`KernelBuilder::built_in_ids`](crate::KernelBuilder::built_in_ids)).
pub(crate) struct CodeAtStart {
    pub(crate) plugins: Option<CodePlugins>,
    pub(crate) built_in: Option<fn(&str) -> bool>,
}

/// Why the code file at `path` does not run where the loader took it for
/// none of its own ([`CodeLoader::load_native`] answered `Ok(None)`): a file
/// of the format before interface 7 was built for an older PaperPaw (Beta 3's
/// plugins: listed so, never loaded and never taken for damaged), one of a
/// format this build does not read needs a newer one, an interface-7 file
/// where the loader runs none is not supported here, anything else is
/// damaged. Not inlined: the header it reads stays out of the frame of the
/// start's loop over the plugins, which the admissions stand on.
#[inline(never)]
pub(crate) fn file_problem(storage: &dyn Storage, path: &str) -> PluginProblem {
    use paperpaw_native::{HEADER_LEN, Header, MAGIC, Problem};
    let mut head = [0u8; HEADER_LEN];
    let mut at = 0;
    while at < head.len() {
        match storage.read_at(path, at as u64, &mut head[at..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => at += n,
        }
    }
    let head = &head[..at];
    match Header::read(head) {
        // Whatever a build of the time said of itself, the format before is
        // not this interface's.
        Ok(header) => PluginProblem::OlderInterface {
            built_for: header.interface.min(paperpaw_sdk::native::INTERFACE - 1),
        },
        Err(Problem::NewerFormat)
            if head.len() >= 6
                && head[..4] == MAGIC
                && u16::from_le_bytes([head[4], head[5]]) == header5::FORMAT =>
        {
            PluginProblem::Unsupported
        }
        Err(problem) => Native::problem_of_file(head, problem),
    }
}

/// What a code plugin that was never installed may not use: the user was
/// never asked about it (Decision 205), so it may not put other code or a
/// firmware on the device, nor change what the device trusts.
pub(crate) const NEVER_ASKED_WITHHELD: [&str; 2] = [
    crate::native_host::commands::dev::FEATURE_INSTALL,
    crate::native_host::commands::dev::FEATURE_FIRMWARE,
];

impl Kernel {
    /// Registers the code plugins found among the storage's plugins, in
    /// the order of their ids, as the configuration (`disabled`) and safe
    /// mode say.
    pub(crate) fn add_code_plugins(
        &mut self,
        mut found: Vec<DataPlugin>,
        disabled: &[String],
        code: CodeAtStart,
    ) {
        paperpaw_sort::sort_by_cached_key(&mut found, |plugin| plugin.manifest().id);
        let CodeAtStart {
            plugins: code,
            built_in,
        } = code;
        if let Some(built_in) = built_in {
            found = self.refuse_built_in_ids(found, disabled, built_in);
        }
        let Some(CodePlugins {
            mut native,
            mut loader,
            safe_mode,
        }) = code
        else {
            let problem = PluginProblem::Unsupported;
            for plugin in found {
                self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
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
        // Plugins whose author's key was revoked (stolen): their code does
        // not run.
        let revoked = paperpaw_sdk::native::keys::revoked(storage.as_ref());
        let host = self.flat_host();
        loader.prepare(&storage);
        for plugin in found {
            let manifest = plugin.manifest();
            // Each try says its own why: what was kept of a declaration cap an
            // earlier try ran into goes, whatever this one ends in.
            let id = self.begin_try(&manifest.id);
            if disabled.iter().any(|d| d == id.as_str()) {
                self.list_data_off(&plugin, manifest);
                self.set_code_info(&id, None);
                continue;
            }
            let path = native.path(plugin.folder());
            // An installed plugin runs only the file installed. Code copied
            // to the card and never installed runs as it lies, without a
            // question (Decision 205), unless an author's key is pinned for
            // its id: a copied file never takes over a signed plugin's id.
            let sha256 = installed.get(id.as_str());
            if sha256.is_none() && crate::native_host::plugin_trust::pinned(storage.as_ref(), id.as_str()) {
                let message =
                    format!("{path}: never installed, and its id has an author's key; not loaded");
                self.warn(WarningSource::Plugin(id.clone()), None, message);
                let problem = PluginProblem::NotInstalled;
                self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
                continue;
            }
            // Per plugin: a revocation holds for the author pinned for
            // this plugin, not for others its key signed.
            if revoked.iter().any(|r| r.plugin == id.as_str()) {
                let message = format!("{path}: its author's key is revoked; not loaded");
                self.warn(WarningSource::Plugin(id.clone()), None, message);
                let problem = PluginProblem::Revoked;
                self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
                continue;
            }
            let file = CodeFile {
                id: &id,
                path: &path,
                sha256,
                host: &host,
            };
            if self.load_native_code(loader.as_mut(), storage.as_ref(), &file, &plugin, disabled) {
                continue;
            }
            // Not a file the loader runs: the file itself says why.
            let problem = file_problem(storage.as_ref(), &path);
            let message = format!("{path}: not loaded ({problem:?})");
            self.warn(WarningSource::Plugin(id.clone()), None, message);
            self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
        }
        drop(installed);
        loader.finish(storage.as_ref());
        self.ask_hyphenation_sink(loader.as_mut(), &storage);
        self.shared.native = Some(native);
    }

    /// A card folder with code whose id is a plugin's built into the program
    /// (`built_in`, [`KernelBuilder::built_in_ids`](crate::KernelBuilder::built_in_ids))
    /// is refused for its id before anything else is asked of it, whatever
    /// its file, switch or start (a plugin of Beta 3 that is built in now):
    /// nothing of it loads, not its translations, safe mode does not switch
    /// its id off, and its row is the built-in plugin's once that registers.
    /// The folders left.
    fn refuse_built_in_ids(
        &mut self,
        found: Vec<DataPlugin>,
        disabled: &[String],
        built_in: fn(&str) -> bool,
    ) -> Vec<DataPlugin> {
        let (refused, found): (Vec<DataPlugin>, Vec<DataPlugin>) = found
            .into_iter()
            .partition(|plugin| built_in(plugin.manifest().id.as_str()));
        for plugin in refused {
            let id = self.begin_try(&plugin.manifest().id);
            let problem = PluginProblem::Mismatch;
            let message = format!(
                "{}: a plugin built in has this id; not loaded ({problem:?})",
                plugin.folder()
            );
            self.warn(WarningSource::Plugin(id), None, message);
            self.list_code_plugin(&plugin, disabled, PluginState::Failed, problem);
        }
        found
    }

    /// Asks the loader whether `file` is a native library and, if so, admits it
    /// ([`Self::add_native_plugin`]) or lists why not; `true` when it was one.
    fn load_native_code(
        &mut self,
        loader: &mut dyn CodeLoader,
        storage: &dyn Storage,
        file: &CodeFile<'_>,
        plugin: &DataPlugin,
        disabled: &[String],
    ) -> bool {
        match loader.load_native(storage, file) {
            Ok(None) => false,
            Ok(Some(admission)) => {
                self.add_native_plugin(plugin, disabled, admission, file.sha256.is_none());
                true
            }
            Err(problem) => {
                let message = format!("{}: not loaded ({problem:?})", file.path);
                self.warn(WarningSource::Plugin(file.id.clone()), None, message);
                self.list_code_plugin(plugin, disabled, PluginState::Failed, problem);
                true
            }
        }
    }

    /// A native plugin the shell hosted: admitted through the kernel before
    /// anything is registered of its folder's data, so a refusal leaves
    /// nothing of it behind (it is listed as failed, with why). Its code runs
    /// as the owner `plugin`'s id names. One `unsigned` (never installed,
    /// so never asked about) may not install plugins or firmware
    /// ([`NEVER_ASKED_WITHHELD`]): one that declares either is not loaded
    /// until it is installed.
    fn add_native_plugin(
        &mut self,
        plugin: &DataPlugin,
        disabled: &[String],
        admission: Box<dyn NativeAdmission>,
        unsigned: bool,
    ) {
        let manifest = plugin.manifest();
        let id = manifest.id.clone();
        let taken = self.shared.plugins.get(id.as_str()).is_some()
            || crate::plugins::RESERVED_IDS.contains(&id.as_str());
        let withheld = unsigned
            && NEVER_ASKED_WITHHELD
                .iter()
                .any(|key| admission.declares(key));
        let admitted = if taken {
            Err(PluginProblem::Mismatch)
        } else if withheld {
            Err(PluginProblem::NotInstalled)
        } else {
            admission.admit(self, plugin.folder())
        };
        match admitted {
            Ok(()) => {
                self.register_data(plugin, manifest);
                self.set_code_info(&id, None);
                if unsigned {
                    self.mark_unsigned(&id);
                }
            }
            Err(problem) if withheld => {
                let message = format!(
                    "{}: never installed, and it declares installing plugins or firmware; not loaded ({problem:?})",
                    plugin.folder()
                );
                self.warn(WarningSource::Plugin(id), None, message);
                self.list_code_plugin(plugin, disabled, PluginState::Failed, problem);
            }
            Err(problem) => {
                let message = format!("{}: not loaded ({problem:?})", plugin.folder());
                self.warn(WarningSource::Plugin(id), None, message);
                self.list_code_plugin(plugin, disabled, PluginState::Failed, problem);
            }
        }
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
    /// off; for one refused for its id (`Mismatch`) not even those
    /// (#1424 R1).
    fn list_code_plugin(
        &mut self,
        plugin: &DataPlugin,
        disabled: &[String],
        state: PluginState,
        problem: PluginProblem,
    ) {
        let manifest = plugin.manifest();
        let id = manifest.id.clone();
        if problem == PluginProblem::Mismatch {
            self.list_bare(manifest);
        } else {
            self.list_data_off(plugin, manifest);
        }
        let on = !disabled.iter().any(|d| d == id.as_str());
        if let Some(info) = self.shared.plugin_mut(id.as_str()) {
            info.enabled = on;
            if on {
                info.state = state;
            }
        }
        self.set_code_info(&id, Some(problem));
    }

    /// Keeps that `id` runs code that was never installed, for its row in
    /// the plugin manager (`reg_plugin.flags` bit 6, "not signed").
    fn mark_unsigned(&mut self, id: &Id) {
        let kept = self
            .shared
            .registry
            .native_declarations
            .as_deref()
            .is_some_and(|directory| directory.mark_unsigned(id.as_str()));
        if !kept {
            let message = format!("{id}: runs unsigned; no room to say so in its row");
            self.warn(WarningSource::Plugin(id.clone()), None, message);
        }
    }

    fn set_code_info(&mut self, id: &Id, problem: Option<PluginProblem>) {
        if let Some(info) = self.shared.plugin_mut(id.as_str()) {
            info.kind = PluginKind::Code;
            info.problem = problem;
        }
    }
}

#[cfg(test)]
#[path = "code_plugin_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "code_plugin_format5_tests.rs"]
mod format5_tests;

#[cfg(test)]
#[path = "code_plugin_startup_tests.rs"]
mod startup_tests;

#[cfg(test)]
#[path = "code_plugin_migration_tests.rs"]
mod migration_tests;
