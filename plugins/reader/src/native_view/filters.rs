//! The text filters of `reader.filter@1` for the book on show
//! (`native_filter`): asked once when the book opens and again when a setting
//! of another plugin changes; their signature is part of the layout's key, so
//! pages laid out under other rules are not taken for these.
use super::{Body, Host, LayoutKey, Open, Setup, View};
use crate::native_filter;
use alloc::vec::Vec;

impl Open {
    /// Asks the rules of the book; a refusal leaves it unfiltered.
    pub(super) fn ask_filters(&mut self, host: &Host<'_>) {
        // The old rules go first: the two are never held at once.
        self.filters = Vec::new();
        if let Ok(filters) = native_filter::ask(host, &self.path, &self.language) {
            self.filters = filters;
        }
    }

    /// What a page laid out with `setup` and this book's rules depends on.
    pub(super) fn key(&self, setup: &Setup) -> LayoutKey {
        LayoutKey {
            filtered: native_filter::signature(&self.filters),
            ..setup.key()
        }
    }
}

impl View {
    /// A setting of another plugin changed: the rules are asked again.
    /// Whether they are others now (the page is to be laid out anew).
    pub fn refilter(&mut self, host: &Host<'_>) -> bool {
        let Body::Open(open) = &mut self.body else {
            return false;
        };
        let before = native_filter::signature(&open.filters);
        let asked = native_filter::ask(host, &open.path, &open.language);
        let old = core::mem::take(&mut open.filters);
        open.filters = native_filter::renewed(old, asked);
        native_filter::signature(&open.filters) != before
    }
}
