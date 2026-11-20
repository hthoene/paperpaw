//! The open book as the script hint reads it (`native_script_hint`): the packs
//! of letters it asks for when it opens, and those it asked for while open.
use super::{Body, Documents, Host, Open, View};
use crate::native_script_hint::{self as hint, Asked, Book};
use alloc::string::String;

/// What the script hint keeps of an open book.
#[derive(Debug, Default)]
pub(super) struct Packs {
    /// The packs it asked for while open (`native_script_hint`).
    asked: Asked,
    /// What its first blocks lack, read at this opening: kept in the step
    /// after its page is shown, not before (a card write; #403).
    scanned: Option<String>,
}

impl Open {
    /// The packs of letters the reading font lacks, asked for once a book;
    /// `script` is that of the book's declared language. What its first
    /// blocks lack, where they were read now, is kept after its first page
    /// is shown (`packs_step`).
    pub(super) fn ask_packs(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        script: Option<&'static str>,
    ) {
        let book = Book {
            id: self.id,
            sections: self.sections,
            whole_pages: self.whole_pages,
            path: &self.path,
            script,
            language: &self.language,
            conversion: self.conversion,
        };
        let (asked, scanned) = hint::ask_part(host, docs, &book, hint::Part::Opening);
        self.packs = Packs { asked, scanned };
    }
}

impl View {
    /// Whether what the book's first blocks lack is still to be kept.
    #[must_use]
    pub fn packs_due(&self) -> bool {
        matches!(&self.body, Body::Open(open) if open.packs.scanned.is_some())
    }

    /// The step after the first page is shown: what the book's first blocks
    /// lack is kept, so that the next opening does not read them again.
    pub fn packs_step(&mut self, host: &Host<'_>) {
        if let Body::Open(open) = &mut self.body
            && let Some(text) = open.packs.scanned.take()
        {
            hint::keep_scanned(host, &open.path, &text);
        }
    }

    /// The open book as the script hint reads it, with the script of its
    /// language (the UI's where it declares none, as Beta 3 asked again once
    /// letters came).
    pub(crate) fn hint_book(&self) -> Option<Book<'_>> {
        match &self.body {
            Body::Open(open) => Some(Book {
                id: open.id,
                sections: open.sections,
                whole_pages: open.whole_pages,
                path: &open.path,
                script: hint::declared(&open.language),
                language: &open.language,
                conversion: open.conversion,
            }),
            Body::Message(_) | Body::Preparing(_) => None,
        }
    }

    /// The packs of letters the open book asked for, to take an answer off.
    pub(crate) fn packs_asked(&mut self) -> Option<&mut Asked> {
        match &mut self.body {
            Body::Open(open) => Some(&mut open.packs.asked),
            Body::Message(_) | Body::Preparing(_) => None,
        }
    }
}
