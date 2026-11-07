//! The data plugins' hyphenation tables as the kernel registered them, kept
//! for the shell's `text.hyphenate` service ([`HyphenTables`]): the shell hands
//! the kernel a sink once ([`CodeLoader::hyphenation_sink`]
//! (crate::code_plugin::CodeLoader::hyphenation_sink)); from then on every
//! plugin added or removed, and every change of the card, reaches it.
use crate::code_plugin::CodeLoader;
use crate::native_host::HyphenTables;
use alloc::{collections::TryReserveError, rc::Rc, string::String, vec::Vec};
use paperpaw_platform::Storage;

/// One declared table: the plugin, the language tag, the card path.
struct Declared {
    plugin: String,
    tag: String,
    path: String,
}

/// What the kernel registered, and where it goes.
pub(crate) struct HyphenFeed {
    declared: Vec<Declared>,
    sink: Option<HyphenTables>,
}

impl HyphenFeed {
    pub(crate) const fn new() -> Self {
        Self {
            declared: Vec::new(),
            sink: None,
        }
    }

    /// Asks the shell's loader for its sink: it gets everything declared so
    /// far, and what follows.
    pub(crate) fn ask(&mut self, loader: &mut dyn CodeLoader, storage: &Rc<dyn Storage>) {
        if let Some(sink) = loader.hyphenation_sink(storage) {
            self.attach(sink);
        }
    }

    pub(crate) fn attach(&mut self, sink: HyphenTables) {
        self.sink = Some(sink);
        self.push();
    }

    /// The tables of data plugin `plugin` (language tag, file relative to
    /// `folder`). All or none.
    ///
    /// # Errors
    /// No memory; nothing was added.
    pub(crate) fn add<'t>(
        &mut self,
        plugin: &str,
        folder: &str,
        tables: impl ExactSizeIterator<Item = (&'t str, &'t str)>,
    ) -> Result<(), TryReserveError> {
        let mut new = Vec::new();
        new.try_reserve_exact(tables.len())?;
        for (tag, path) in tables {
            new.push(Declared {
                plugin: copy(plugin)?,
                tag: copy(tag)?,
                path: join(folder, path)?,
            });
        }
        self.declared.try_reserve(new.len())?;
        self.declared.extend(new);
        self.push();
        Ok(())
    }

    /// The tables of `plugin` are gone.
    pub(crate) fn remove(&mut self, plugin: &str) {
        let before = self.declared.len();
        self.declared.retain(|d| d.plugin != plugin);
        if self.declared.len() != before {
            self.push();
        }
    }

    /// The card may hold other files than before.
    pub(crate) fn card_changed(&self) {
        if let Some(sink) = &self.sink {
            sink.card_changed();
        }
    }

    /// Tells the sink what is declared now. A sink that cannot take it keeps
    /// the declarations it had.
    fn push(&self) {
        if let Some(sink) = &self.sink {
            let _ = sink.declare_plugins(
                self.declared
                    .iter()
                    .map(|d| (d.tag.as_str(), d.path.as_str(), d.plugin.as_str())),
            );
        }
    }
}

fn copy(text: &str) -> Result<String, TryReserveError> {
    let mut owned = String::new();
    owned.try_reserve_exact(text.len())?;
    owned.push_str(text);
    Ok(owned)
}

/// `folder`, a slash and `path`, as the kernel's own tables join them.
fn join(folder: &str, path: &str) -> Result<String, TryReserveError> {
    let mut owned = String::new();
    owned.try_reserve_exact(folder.len() + 1 + path.len())?;
    owned.push_str(folder);
    owned.push('/');
    owned.push_str(path);
    Ok(owned)
}

#[cfg(test)]
#[path = "hyphen_feed_tests.rs"]
mod tests;
