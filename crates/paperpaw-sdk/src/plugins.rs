//! The plugins the kernel knows about: running, switched off or failed.
//!
//! Plugin managers read this list through [`Env::plugins`](crate::Env) and
//! switch plugins with [`Command::SetPluginEnabled`](crate::Command). The
//! change is saved to `paperpaw.ini` and applies after a restart
//! ([`Command::Restart`](crate::Command)).

use alloc::format;
use alloc::vec::Vec;

use paperpaw_i18n::Translator;

use crate::builtin::{fonts, slots};
use crate::id::Id;
use crate::plugin::Manifest;

/// What [`RequiredFor`] can name: the standard slots and the UI font.
static ESSENTIALS: [Id; 6] = [
    slots::HOME,
    slots::READER,
    slots::LIBRARY,
    slots::MENU,
    slots::SETTINGS,
    fonts::UI,
];

/// The standard slots (and `core.font-ui` for "all fonts") that only one
/// plugin provides. A bit set: no heap memory per plugin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequiredFor(u8);

impl RequiredFor {
    /// Adds `id`; `false` if it is not a standard slot or the UI font.
    pub fn insert(&mut self, id: &Id) -> bool {
        match ESSENTIALS.iter().position(|e| e == id) {
            Some(bit) => {
                self.0 |= 1 << bit;
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, id: &Id) -> bool {
        ESSENTIALS
            .iter()
            .position(|e| e == id)
            .is_some_and(|bit| self.0 & (1 << bit) != 0)
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &'static Id> + '_ {
        ESSENTIALS
            .iter()
            .enumerate()
            .filter(|(bit, _)| self.0 & (1 << bit) != 0)
            .map(|(_, id)| id)
    }
}

/// Whether a plugin runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginState {
    /// Loaded and running.
    Running,
    /// Switched off in the configuration; not loaded.
    Off,
    /// Could not be loaded (see the kernel's warnings).
    Failed,
}

/// What a plugin is made of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginKind {
    /// Built into PaperPaw.
    #[default]
    BuiltIn,
    /// A folder of data files on the storage (a theme, a translation …).
    Data,
    /// A folder on the storage with native code ([`native`](crate::native)).
    Code,
}

/// Why a code plugin does not run, for plugin managers to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginProblem {
    /// Its code was built for another version of PaperPaw or another
    /// device: it waits for a build for this one.
    OtherBuild,
    /// Its code's file is damaged, or not one this PaperPaw reads.
    Damaged,
    /// Its code does not fit its folder (another plugin's id).
    Mismatch,
    /// There is no room left for its code (the X4's flash).
    NoRoom,
    /// This PaperPaw loads no code plugins.
    Unsupported,
    /// This start went without code plugins
    /// ([`SafeMode`](crate::native::SafeMode)).
    SafeMode,
    /// Safe mode switched it off: PaperPaw did not start with it.
    Crashed,
    /// Its code was never installed (by a store, or from the web
    /// flasher): a file copied into its folder is not run.
    NotInstalled,
    /// Its code is not the file installed: changed since.
    Changed,
    /// Its code was built for an older PaperPaw (an older plugin
    /// interface, or shared functions this PaperPaw changed since): it
    /// waits for a new build, which the store may have.
    OlderInterface {
        /// The plugin interface it was built for (its fingerprint says).
        built_for: u16,
    },
    /// Its code needs a newer PaperPaw: an update of PaperPaw runs it.
    NeedsNewerPaperPaw,
}

/// A plugin and its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub manifest: Manifest,
    pub state: PluginState,
    /// Switched on in the configuration. Differs from `state` until the
    /// next start when the user has just switched the plugin.
    pub enabled: bool,
    /// Core slots no other plugin fills (e.g. `core.settings`), or
    /// `core.font-ui` if the plugin provides every font. Switching the
    /// plugin off would leave them empty, so it stays on until another
    /// plugin can take over.
    pub required_for: RequiredFor,
    pub kind: PluginKind,
    /// Why it does not run, where its code is the reason.
    pub problem: Option<PluginProblem>,
    /// A newer version installed while its code runs
    /// ([`Command::InstallPlugin`](crate::Command::InstallPlugin)): it
    /// takes over at the next start.
    pub update: Option<crate::Version>,
}

impl PluginInfo {
    /// The name in the current language: the plugin's translation of
    /// `<id>.plugin.name`, or the manifest's name.
    pub fn name<'a>(&'a self, i18n: &'a Translator) -> &'a str {
        i18n.get(&format!("{}.plugin.name", self.manifest.id))
            .unwrap_or(&self.manifest.name)
    }

    /// The description in the current language: the plugin's translation
    /// of `<id>.plugin.description`, or the manifest's description.
    pub fn description<'a>(&'a self, i18n: &'a Translator) -> &'a str {
        i18n.get(&format!("{}.plugin.description", self.manifest.id))
            .unwrap_or(&self.manifest.description)
    }

    /// How it works, at length, in the current language (else in
    /// English): the plugin's own text ([`Manifest::how`]), else its
    /// translation of `<id>.plugin.how` (a data plugin's); plain text,
    /// paragraphs apart by an empty line. Shown by the plugin manager as
    /// *How it works*.
    pub fn how<'a>(&'a self, i18n: &'a Translator) -> Option<&'a str> {
        let language = i18n.language();
        let base = language.split('-').next().unwrap_or(language);
        let own = [language, base, "en"].into_iter().find_map(|wanted| {
            self.manifest
                .how
                .iter()
                .find(|(written, _)| *written == wanted)
                .map(|(_, text)| *text)
        });
        own.or_else(|| i18n.get(&format!("{}.plugin.how", self.manifest.id)))
            .filter(|text| !text.trim().is_empty())
    }

    /// Whether the user may switch the plugin off.
    pub fn can_switch_off(&self) -> bool {
        self.manifest.id != crate::builtin::CORE_PLUGIN && self.required_for.is_empty()
    }

    /// Whether a restart is needed to apply the user's choice.
    pub fn restart_needed(&self) -> bool {
        if self.update.is_some() {
            return true;
        }
        match self.state {
            PluginState::Running => !self.enabled,
            PluginState::Off => self.enabled,
            PluginState::Failed => false,
        }
    }
}

/// Every plugin given to the kernel, in registration order.
#[derive(Debug, Default)]
pub struct Plugins {
    list: Vec<PluginInfo>,
}

impl Plugins {
    pub const fn new() -> Self {
        Self { list: Vec::new() }
    }

    pub fn iter(&self) -> impl Iterator<Item = &PluginInfo> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// PaperPaw's kernel as it runs here ([`CORE_PLUGIN`](crate::builtin::CORE_PLUGIN)):
    /// its manifest says the version of PaperPaw and of its SDK, which a
    /// plugin built with another SDK may not have.
    pub fn kernel(&self) -> Option<&PluginInfo> {
        self.get(crate::builtin::CORE_PLUGIN.as_str())
    }

    pub fn get(&self, id: &str) -> Option<&PluginInfo> {
        self.list.iter().find(|p| p.manifest.id.as_str() == id)
    }

    /// The plugin that added `id` (an action, a screen, a setting …): what
    /// a plugin adds is in its namespace, the registrar refuses anything
    /// else (see [`Registrar`](crate::Registrar)).
    pub fn owner(&self, id: &Id) -> Option<&PluginInfo> {
        self.get(id.namespace())
    }

    /// Whether a plugin was switched since the start.
    pub fn restart_needed(&self) -> bool {
        self.list.iter().any(PluginInfo::restart_needed)
    }

    /// Adds a plugin (kernel only).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn push(&mut self, info: PluginInfo) {
        self.list.push(info);
    }

    /// Removes a plugin (kernel only), e.g. a data plugin removed while
    /// running.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn remove(&mut self, id: &str) -> Option<PluginInfo> {
        let index = self
            .list
            .iter()
            .position(|p| p.manifest.id.as_str() == id)?;
        Some(self.list.remove(index))
    }

    /// Mutable access for the kernel.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn get_mut(&mut self, id: &str) -> Option<&mut PluginInfo> {
        self.list.iter_mut().find(|p| p.manifest.id.as_str() == id)
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.list.shrink_to_fit();
    }
}

#[cfg(test)]
mod tests {
    use alloc::borrow::Cow;
    use alloc::vec::Vec;

    use super::*;
    use crate::Version;

    fn info(id: &'static str, state: PluginState, enabled: bool) -> PluginInfo {
        PluginInfo {
            manifest: Manifest {
                id: Id::new_static(id),
                name: Cow::Borrowed("Name"),
                description: Cow::Borrowed("Does things."),
                version: Version::new(1, 0, 0),
                uses: crate::Uses::NOTHING,
                how: &[],
            },
            state,
            enabled,
            required_for: RequiredFor::default(),
            kind: PluginKind::BuiltIn,
            problem: None,
            update: None,
        }
    }

    #[test]
    fn restart_is_needed_only_for_switched_plugins() {
        let mut plugins = Plugins::new();
        plugins.push(info("a", PluginState::Running, true));
        plugins.push(info("b", PluginState::Failed, false));
        assert!(!plugins.restart_needed());
        plugins.get_mut("a").unwrap().enabled = false;
        assert!(plugins.restart_needed());
    }

    #[test]
    fn names_fall_back_to_the_manifest() {
        let mut i18n = Translator::new();
        let catalog = paperpaw_i18n::Catalog::parse(Cow::Borrowed(
            "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\na.plugin.name = Ah\n",
        ))
        .unwrap();
        i18n.add(catalog);
        i18n.set_language("de");
        let a = info("a", PluginState::Running, true);
        assert_eq!(a.name(&i18n), "Ah");
        assert_eq!(a.description(&i18n), "Does things.");
        let core = info("core", PluginState::Running, true);
        assert!(!core.can_switch_off());
        assert!(a.can_switch_off());
    }

    #[test]
    fn required_for_holds_standard_slots_only() {
        let mut needs = RequiredFor::default();
        assert!(needs.insert(&slots::MENU));
        assert!(needs.insert(&fonts::UI));
        assert!(!needs.insert(&Id::new_static("x.slot")));
        assert!(needs.contains(&slots::MENU));
        assert!(!needs.contains(&slots::HOME));
        let ids: Vec<&Id> = needs.iter().collect();
        assert_eq!(ids, [&slots::MENU, &fonts::UI]);
    }
}
