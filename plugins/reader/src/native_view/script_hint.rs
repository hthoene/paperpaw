//! The open book as the script hint reads it (`native_script_hint`): the packs
//! of letters it asks for when it opens, and those it asked for while open.
use super::{Body, Documents, Host, Open, View};
use crate::native_script_hint::{self as hint, Asked, Book};

impl Open {
    /// The packs of letters the reading font lacks, asked for once a book;
    /// `script` is that of the book's declared language.
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
        };
        self.packs_asked = hint::ask(host, docs, &book);
    }
}

impl View {
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
            }),
            Body::Message(_) | Body::Preparing(_) => None,
        }
    }

    /// The packs of letters the open book asked for, to take an answer off.
    pub(crate) fn packs_asked(&mut self) -> Option<&mut Asked> {
        match &mut self.body {
            Body::Open(open) => Some(&mut open.packs_asked),
            Body::Message(_) | Body::Preparing(_) => None,
        }
    }
}
