//! A language the firmware carries as a language pack instead of built-in
//! texts (German, #251): the program holds the pack compressed, and writes it
//! to the card as the data plugin `plugin` when the language is chosen or
//! already is at a start (an update from a firmware that had the language
//! built in). Without Wi-Fi, once; after that the pack is an ordinary data
//! plugin: the store updates it, the plugin manager removes it.

use alloc::format;
use alloc::string::String;
use paperpaw_i18n::Catalog;

use crate::Kernel;

/// Where a language pack's catalog goes as it is unpacked
/// ([`LanguageSeed::catalog`]): given pieces in order, `false` to stop.
pub type Sink<'a> = dyn FnMut(&[u8]) -> bool + 'a;

/// A language pack carried by the program (`KernelBuilder::language_seed`).
#[derive(Clone, Copy)]
pub struct LanguageSeed {
    /// The language's tag, as the setting says it (`de`).
    pub tag: &'static str,
    /// Its name in the language (`Deutsch`), listed beside the others before
    /// the pack is on the card.
    pub name: &'static str,
    /// Its plural rule (`[meta] plural`).
    pub plural: &'static str,
    /// The data plugin the pack is (`language-de`).
    pub plugin: &'static str,
    /// The pack's `plugin.ini`.
    pub plugin_ini: &'static str,
    /// Gives the compiled catalog (`i18n/<tag>.ppcat`) to `sink` piece by
    /// piece, in order, unpacked as it goes; `false` where `sink` said stop
    /// or it cannot be unpacked.
    pub catalog: fn(sink: &mut Sink<'_>) -> bool,
}

impl core::fmt::Debug for LanguageSeed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LanguageSeed")
            .field("tag", &self.tag)
            .field("plugin", &self.plugin)
            .finish_non_exhaustive()
    }
}

impl LanguageSeed {
    /// A catalog with the language's meta only: the language is offered
    /// (settings, first start) before its pack is on the card.
    pub(crate) fn listing(&self) -> Option<Catalog> {
        let source = format!(
            "[meta]\nlanguage = {}\nname = {}\nplural = {}\n",
            self.tag, self.name, self.plural
        );
        // Written here, as the catalog parser reads it.
        Catalog::parse(source).ok()
    }
}

impl Kernel {
    /// Writes the language pack the program carries and loads it, when its
    /// language is the interface's and no plugin of its id is there.
    pub(crate) fn seed_language(&mut self) {
        let Some(seed) = self.shared.language_seed else {
            return;
        };
        if self.shared.translator.language() != seed.tag
            || self.shared.plugins.get(seed.plugin).is_some()
            || self
                .shared
                .data_folders
                .iter()
                .any(|(id, _)| id.as_str() == seed.plugin)
        {
            return;
        }
        let dir = self
            .shared
            .plugin_dirs
            .first()
            .map_or(paperpaw_sdk::builtin::PLUGIN_DIR, String::as_str);
        let target = paperpaw_sdk::files::join(dir, seed.plugin);
        let storage = self.shared.storage.clone();
        // Not a plugin where the folder is (a cut install, another's): left.
        if storage.exists(&target) {
            return;
        }
        // Made aside and moved into place at once: a start cut short leaves
        // no half of a pack for the scan to find.
        let aside = paperpaw_sdk::files::join(dir, &format!(".{}.seed", seed.plugin));
        let catalogs = paperpaw_sdk::files::join(&aside, "i18n");
        let file = paperpaw_sdk::files::join(&catalogs, &format!("{}.ppcat", seed.tag));
        let _ = storage.remove_dir_all(&aside);
        let _ = storage.create_dir(dir);
        let _ = storage.create_dir(&aside);
        let _ = storage.create_dir(&catalogs);
        let mut first = true;
        let mut written = (seed.catalog)(&mut |piece| {
            let done = if first {
                storage.write(&file, piece)
            } else {
                storage.append(&file, piece)
            };
            first = false;
            done.is_ok()
        });
        written = written
            && storage
                .write(
                    &paperpaw_sdk::files::join(&aside, "plugin.ini"),
                    seed.plugin_ini.as_bytes(),
                )
                .is_ok()
            && storage.rename(&aside, &target).is_ok();
        if !written {
            let _ = storage.remove_dir_all(&aside);
            let problem = format!("{}: could not write the language pack", seed.plugin);
            self.warn_runtime(problem);
            return;
        }
        self.load_plugin(&target);
    }
}
