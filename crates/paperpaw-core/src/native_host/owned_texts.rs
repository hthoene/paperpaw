//! What of interface-7 plugins' declarations the kernel keeps as copies on the
//! heap where the program holds them (#248): for the test that guards the
//! saving of borrowed ids and titles (`paperpaw-app`'s linked tables test).

use alloc::{borrow::Cow, format, string::String, vec::Vec};
use paperpaw_sdk::Id;

impl crate::Kernel {
    /// The ids and titles of interface-7 plugins' actions, screens and
    /// settings that are copies on the heap, each as `kind id`; empty where
    /// every one is kept in the program (a linked plugin with its tables).
    pub fn owned_native_texts(&self) -> Vec<String> {
        let shared = &self.shared;
        let plugins: Vec<&Id> = shared.native_instances.plugin_ids().collect();
        let native = |id: &Id| plugins.iter().any(|plugin| id.is_owned_by(plugin));
        let mut owned = Vec::new();
        let mut note = |kind: &str, id: &Id, title_kept: bool| {
            if id.kept().is_none() {
                owned.push(format!("{kind} id {id}"));
            }
            if !title_kept {
                owned.push(format!("{kind} title {id}"));
            }
        };
        for action in shared
            .actions
            .iter()
            .filter(|action| action.native().is_some())
        {
            let title = matches!(action.title_key(), Cow::Borrowed(_));
            note("action", action.id(), title);
        }
        for titled in shared.registry.screens.native_titles() {
            note(
                "screen",
                titled.id(),
                matches!(titled.title_key(), Cow::Borrowed(_)),
            );
        }
        // Those kept only: what a plugin describes of its own is not on the heap.
        for spec in shared.settings.kept_specs().filter(|spec| native(&spec.id)) {
            note("setting", &spec.id, spec.title.kept().is_some());
        }
        for group in shared.settings.groups().filter(|group| native(&group.id)) {
            note("group", &group.id, matches!(group.title, Cow::Borrowed(_)));
        }
        owned
    }
}
