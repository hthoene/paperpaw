//! The `core.format` providers of the crate's formats, one constructor each.
use alloc::boxed::Box;

use crate::{
    CbzDocument, EpubDocument, Fb2Document, HtmlDocument, MarkdownDocument, PLUGIN, is_epub,
    provider,
};
use paperpaw_sdk::document::{Document, DocumentError};

/// The `core.format` provider of EPUB books (`epub`): sniffed by its
/// `mimetype` entry, the opened file named by the call's `text`; the plugins'
/// stylesheets apply after the book's own.
#[must_use]
pub fn epub_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::data::plugin_dir(PLUGIN.as_str());
    provider::FormatProvider::new(
        "format-epub.epub",
        |storage, name, options| {
            EpubDocument::open_options(storage, name, options)
                .map(|d| Box::new(d) as Box<dyn Document>)
        },
        dir,
        is_epub,
    )
    .with_plugin_css()
}

/// The `core.format` provider of fixed-layout EPUB books of pictures (a page
/// each, shown whole). It asks before it opens a book (`FormatDecl::asks`,
/// #242): any other EPUB goes on to the next format.
#[must_use]
pub fn epub_plus_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::data::plugin_dir(PLUGIN.as_str());
    provider::FormatProvider::new(
        "format-epub-plus.epub",
        |storage, name, options| match crate::fixed::open(storage, name, options)? {
            Some(d) => Ok(Box::new(d) as Box<dyn Document>),
            None => Err(DocumentError::UnsupportedFormat),
        },
        dir,
        is_epub,
    )
    .with_claims(|storage, name| crate::fixed::claims(storage, name))
}

/// The `core.format` provider of FictionBooks (`fb2`, `fb2.zip`): a
/// `siblings` format, the opened file named by the call's `text`.
#[must_use]
pub fn fb2_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::data::plugin_dir(PLUGIN.as_str());
    provider::FormatProvider::new(
        "format-fb2.fb2",
        |storage, name, options| {
            Fb2Document::open_options(storage, name, options)
                .map(|d| Box::new(d) as Box<dyn Document>)
        },
        dir,
        |_| false,
    )
}

/// The `core.format` provider of comic book archives (`cbz`): pages are
/// pictures, shown whole.
#[must_use]
pub fn cbz_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::data::plugin_dir(PLUGIN.as_str());
    provider::FormatProvider::new(
        "format-cbz.cbz",
        |storage, name, options| {
            CbzDocument::open_options(storage, name, options)
                .map(|d| Box::new(d) as Box<dyn Document>)
        },
        dir,
        |_| false,
    )
}

/// The `core.format` provider of HTML files (`htm`, `html`, `xhtml`): a
/// `siblings` format, the opened file named by the call's `text`.
#[must_use]
pub fn html_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::data::plugin_dir(PLUGIN.as_str());
    provider::FormatProvider::new(
        "format-html.html",
        |storage, name, options| {
            HtmlDocument::open(storage, name, options.publisher_styles)
                .map(|d| Box::new(d) as Box<dyn Document>)
        },
        dir,
        |_| false,
    )
}

/// The `core.format` provider of Markdown files (`md`, `markdown`): a
/// `siblings` format, the opened file named by the call's `text`.
#[must_use]
pub fn markdown_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::data::plugin_dir(PLUGIN.as_str());
    provider::FormatProvider::new(
        "format-markdown.markdown",
        |storage, name, options| {
            MarkdownDocument::open(storage, name, options.publisher_styles)
                .map(|d| Box::new(d) as Box<dyn Document>)
        },
        dir,
        |_| false,
    )
}
