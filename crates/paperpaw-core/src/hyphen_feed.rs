//! The data plugins' hyphenation tables as the kernel registered them, handed
//! to the shell's `text.hyphenate` service ([`HyphenTables`]): the shell gives
//! the kernel a sink once ([`CodeLoader::hyphenation_sink`]); from then on
//! every plugin added or removed, and every change of the card, reaches it.
//!
//! The list itself is the SDK's (`Hyphenation::card_tables`): the feed keeps
//! no second copy of the table strings, only the sink.
use crate::code_plugin::CodeLoader;
use crate::native_host::HyphenTables;
use crate::{Kernel, WarningSource};
use alloc::rc::Rc;
use paperpaw_platform::Storage;
use paperpaw_sdk::Hyphenation;

/// Where the declared tables go.
pub(crate) struct HyphenFeed {
    sink: Option<HyphenTables>,
}

impl HyphenFeed {
    pub(crate) const fn new() -> Self {
        Self { sink: None }
    }

    /// The card may hold other files than before.
    pub(crate) fn card_changed(&self) {
        if let Some(sink) = &self.sink {
            sink.card_changed();
        }
    }

    /// Tells the sink what is declared now. A sink that cannot take it keeps
    /// the declarations it had.
    ///
    /// # Errors
    /// The sink's status.
    pub(crate) fn push(&self, tables: &Hyphenation) -> Result<(), i32> {
        match &self.sink {
            Some(sink) => sink.declare_plugins(
                tables
                    .card_tables()
                    .map(|t| (t.tag(), t.path(), t.plugin())),
            ),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub(crate) fn attach(&mut self, sink: HyphenTables) {
        self.sink = Some(sink);
    }
}

/// What makes the sink of a shell without a code loader, from the card.
pub(crate) type MakeSink = alloc::boxed::Box<dyn FnOnce(&Rc<dyn Storage>) -> Option<HyphenTables>>;

impl Kernel {
    /// The sink of a shell without a code loader
    /// ([`KernelBuilder::hyphenation_sink`](crate::KernelBuilder::hyphenation_sink)):
    /// kept only where no loader gave one, and given what is declared so far.
    #[inline(never)]
    pub(crate) fn attach_hyphenation_sink(&mut self, make: MakeSink) {
        if self.shared.hyphen_feed.sink.is_some() {
            return;
        }
        let storage = self.shared.storage.clone();
        if let Some(sink) = make(&storage) {
            self.shared.hyphen_feed.sink = Some(sink);
            self.feed_hyphenation(None);
        }
    }

    /// Asks the shell's loader for its sink; it gets everything declared so
    /// far, and what follows.
    pub(crate) fn ask_hyphenation_sink(
        &mut self,
        loader: &mut dyn CodeLoader,
        storage: &Rc<dyn Storage>,
    ) {
        if let Some(sink) = loader.hyphenation_sink(storage) {
            self.shared.hyphen_feed.sink = Some(sink);
            self.feed_hyphenation(None);
        }
    }

    /// The declared tables changed (of plugin `id`, if one): the sink is told.
    /// One it cannot take is a warning; it keeps what it had.
    pub(crate) fn feed_hyphenation(&mut self, id: Option<&paperpaw_sdk::Id>) {
        let Err(status) = self.shared.hyphen_feed.push(&self.shared.hyphenation) else {
            return;
        };
        let source = id.map_or(WarningSource::Runtime, |id| {
            WarningSource::Plugin(id.clone())
        });
        let problem =
            alloc::format!("hyphenation tables not handed to the shell (status {status})");
        self.warn(source, None, problem);
    }
}

#[cfg(test)]
#[path = "hyphen_feed_tests.rs"]
mod tests;
