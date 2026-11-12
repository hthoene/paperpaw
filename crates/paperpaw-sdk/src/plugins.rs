//! The plugins the kernel knows about: running, switched off or failed.
//!
//! Plugin managers read this list through [`Env::plugins`](crate::Env) and
//! switch plugins with [`Command::SetPluginEnabled`](crate::Command). The
//! change is saved to `paperpaw.ini` and applies after a restart
//! ([`Command::Restart`](crate::Command)).

use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use paperpaw_i18n::Translator;

use crate::builtin::{fonts, slots};
use crate::id::Id;
use crate::plugin::Manifest;

pub mod requires;

/// The most plugins one [`Command::RequestPlugins`](crate::Command::RequestPlugins)
/// asks for; those past it are left out.
pub const MAX_REQUESTED_PLUGINS: usize = 32;

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
    /// A kind a later build adds (a scripted plugin), by its number. Code
    /// that doesn't know it treats it as a plugin on the storage that this
    /// build cannot run.
    Other(u16),
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
    /// The key its author signed it with was revoked (stolen): it does not
    /// run ([`native::keys`](crate::native::keys)).
    Revoked,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

/// A plugin's manifest as the plugin list holds it: one kept in the program
/// (a linked plugin's, no heap) or a copy of its own. Reads as the
/// [`Manifest`] either way (`info.manifest.id`).
#[derive(Clone)]
pub struct ManifestRef(Held);

#[derive(Clone)]
enum Held {
    Fixed(&'static Manifest),
    Owned(Box<Manifest>),
}

impl ManifestRef {
    /// The manifest `manifest`, kept in the program: nothing is copied.
    #[must_use]
    pub const fn fixed(manifest: &'static Manifest) -> Self {
        Self(Held::Fixed(manifest))
    }

    /// `made`, the manifest a plugin has: `kept`, the one kept in the
    /// program, where every field of the two is the same (no heap), else a
    /// copy of `made` of its own. A manifest kept in the program that
    /// differs in any field (its description, what it uses, how it works)
    /// is not taken for it.
    ///
    /// # Errors
    /// `made` back, where the heap has no room for its copy.
    pub fn try_new(made: Manifest, kept: Option<&'static Manifest>) -> Result<Self, Manifest> {
        match kept {
            Some(kept) if *kept == made => Ok(Self::fixed(kept)),
            _ => try_box(made).map(|owned| Self(Held::Owned(owned))),
        }
    }

    /// Whether it is kept in the program ([`ManifestRef::fixed`]), not a copy.
    #[must_use]
    pub const fn is_fixed(&self) -> bool {
        matches!(self.0, Held::Fixed(_))
    }
}

impl core::ops::Deref for ManifestRef {
    type Target = Manifest;

    fn deref(&self) -> &Manifest {
        match &self.0 {
            Held::Fixed(manifest) => manifest,
            Held::Owned(manifest) => manifest,
        }
    }
}

/// `manifest` in a box of its own, or back where the heap has no room for it
/// (`Box::new` would stop PaperPaw instead).
fn try_box(manifest: Manifest) -> Result<Box<Manifest>, Manifest> {
    const { assert!(core::mem::size_of::<Manifest>() > 0) };
    let layout = core::alloc::Layout::new::<Manifest>();
    // SAFETY: a layout of non-zero size (asserted above), as `alloc` needs;
    // null is handled.
    let pointer = unsafe { alloc::alloc::alloc(layout) }.cast::<Manifest>();
    if pointer.is_null() {
        return Err(manifest);
    }
    // SAFETY: fresh storage of `Manifest`'s size and alignment from the
    // global allocator, which `Box` frees with the same layout; written
    // before the box is made.
    unsafe {
        pointer.write(manifest);
        Ok(Box::from_raw(pointer))
    }
}

impl From<Manifest> for ManifestRef {
    /// A copy of its own (on the heap).
    fn from(manifest: Manifest) -> Self {
        Self(Held::Owned(Box::new(manifest)))
    }
}

impl From<&'static Manifest> for ManifestRef {
    fn from(manifest: &'static Manifest) -> Self {
        Self::fixed(manifest)
    }
}

impl core::fmt::Debug for ManifestRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Manifest::fmt(self, f)
    }
}

impl PartialEq for ManifestRef {
    /// By the manifests' fields, wherever each is kept.
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for ManifestRef {}

/// A plugin and its state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginInfo {
    /// What the plugin is: a linked plugin's kept in the program
    /// ([`ManifestRef::fixed`]), any other's a copy.
    pub manifest: ManifestRef,
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
    #[allow(
        clippy::too_many_arguments,
        reason = "all its fields, as the constructor of an open type has them"
    )]
    pub fn new(
        manifest: impl Into<ManifestRef>,
        state: PluginState,
        enabled: bool,
        required_for: RequiredFor,
        kind: PluginKind,
        problem: Option<PluginProblem>,
        update: Option<crate::Version>,
    ) -> PluginInfo {
        PluginInfo {
            manifest: manifest.into(),
            state,
            enabled,
            required_for,
            kind,
            problem,
            update,
        }
    }

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
        // An eighth more, not twice as many: a plugin installed after the
        // start's room for one is used would hold the old list and one
        // twice its size at once (24 KiB on the X4); this way 8 + 9 KiB,
        // still few copies in all.
        if self.list.len() == self.list.capacity() {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "without room for an eighth, push asks for what it needs"
            )]
            let _ = self.list.try_reserve_exact((self.list.len() / 8).max(1));
        }
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

    /// Room for `additional` more plugins, asked for once before they
    /// register: the list then never grows by doubling at the start (the
    /// start's peak on small devices).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn reserve(&mut self, additional: usize) {
        self.list.reserve_exact(additional);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        // Room for one more is kept: a plugin installed after the start
        // goes in without the list being copied, which would hold the old
        // list and one twice its size at once (24 KiB on the X4).
        self.list.shrink_to(self.list.len() + 1);
    }
}

#[cfg(test)]
mod tests {
    use alloc::borrow::Cow;
    use alloc::vec::Vec;

    use super::*;
    use crate::{Uses, Version};

    fn info(id: &'static str, state: PluginState, enabled: bool) -> PluginInfo {
        PluginInfo {
            manifest: ManifestRef::try_new(
                Manifest::new(Id::new_static(id), "Name", Version::new(1, 0, 0))
                    .description("Does things.")
                    .uses(crate::Uses::NOTHING),
                None,
            )
            .unwrap(),
            state,
            enabled,
            required_for: RequiredFor::default(),
            kind: PluginKind::BuiltIn,
            problem: None,
            update: None,
        }
    }

    /// A plugin installed after the start goes in without the list being
    /// copied; later ones grow it by an eighth.
    #[test]
    fn the_list_keeps_room_for_one_more() {
        let mut plugins = Plugins::new();
        for _ in 0..40 {
            plugins.push(info("a", PluginState::Running, true));
        }
        plugins.shrink_to_fit();
        assert_eq!(plugins.list.capacity(), 41);
        plugins.push(info("b", PluginState::Running, true));
        assert_eq!(plugins.list.capacity(), 41);
        // Then by an eighth, not twice as many.
        plugins.push(info("c", PluginState::Running, true));
        assert_eq!(plugins.list.capacity(), 41 + 5);
        for _ in 0..8 {
            plugins.push(info("d", PluginState::Running, true));
        }
        assert_eq!(plugins.list.capacity(), 46 + 5);
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

    /// A linked plugin's manifest, kept in the program (#248, B).
    static LINKED: Manifest = Manifest::fixed(
        Id::new_static("linked"),
        "linked",
        "",
        Version::of_package("0", "6", "0", "beta.3"),
    );

    static HOW: [(&str, &str); 1] = [("en", "What it does.")];

    /// A linked plugin's manifest with every field set (#248, B).
    static FULL: Manifest = Manifest::fixed(
        Id::new_static("full"),
        "Full",
        "Does it all.",
        Version::new(1, 2, 3),
    )
    .uses(Uses::NETWORK)
    .how(&HOW);

    /// The manifest the plugin itself has: the same as `FULL`.
    fn full() -> Manifest {
        Manifest::new(Id::new_static("full"), "Full", Version::new(1, 2, 3))
            .description("Does it all.")
            .uses(Uses::NETWORK)
            .how(&HOW)
    }

    /// The manifest kept in the program stands for the plugin's only where
    /// every field is the same; any other is a copy, so nothing a plugin
    /// declares (what it uses above all) is lost (#248, B; Senior M1, #1372).
    #[test]
    fn a_kept_manifest_stands_only_for_one_the_same_in_every_field() {
        let same = ManifestRef::try_new(full(), Some(&FULL)).unwrap();
        assert!(same.is_fixed());
        assert!(core::ptr::eq(&*same, &FULL));
        static OTHER_HOW: [(&str, &str); 1] = [("en", "Something else.")];
        let differing = [
            full().uses(Uses::NOTHING),
            full().description("Does less."),
            full().how(&OTHER_HOW),
            Manifest::new(Id::new_static("full"), "Fuller", Version::new(1, 2, 3))
                .description("Does it all.")
                .uses(Uses::NETWORK)
                .how(&HOW),
            Manifest::new(Id::new_static("full"), "Full", Version::new(1, 2, 4))
                .description("Does it all.")
                .uses(Uses::NETWORK)
                .how(&HOW),
        ];
        for made in differing {
            let listed = ManifestRef::try_new(made.clone(), Some(&FULL)).unwrap();
            assert!(!listed.is_fixed(), "{made:?}");
            assert_eq!(*listed, made);
        }
        // No manifest kept: a copy.
        assert!(!ManifestRef::try_new(full(), None).unwrap().is_fixed());
    }

    #[test]
    fn a_manifest_kept_in_the_program_is_listed_without_a_copy() {
        let mut plugins = Plugins::new();
        plugins.push(PluginInfo::new(
            &LINKED,
            PluginState::Running,
            true,
            RequiredFor::default(),
            PluginKind::BuiltIn,
            None,
            None,
        ));
        let listed = plugins.get("linked").unwrap();
        assert!(listed.manifest.is_fixed());
        assert!(core::ptr::eq(&*listed.manifest, &LINKED));
        // It reads as the same manifest made at run time, which is a copy.
        let made = Manifest::new(
            Id::new_static("linked"),
            "linked",
            Version::parse("0.6.0-beta.3").unwrap(),
        );
        let copy = ManifestRef::try_new(made.clone(), None).unwrap();
        assert!(!copy.is_fixed());
        assert_eq!(*listed.manifest, made);
        assert_eq!(listed.manifest, copy);
        assert_eq!(
            alloc::format!("{}", listed.manifest.version),
            "0.6.0-beta.3"
        );
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn a_listed_plugin_holds_its_manifest_by_reference() {
        // The plugin list's row: a manifest (80 bytes here) held inline was
        // most of it (#248, B).
        assert!(core::mem::size_of::<PluginInfo>() <= 40);
    }
}
