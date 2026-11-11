//! The `core.format` providers of the crate's formats, one constructor each.
use alloc::boxed::Box;

use crate::{
    CbzDocument, EpubDocument, Fb2Document, HtmlDocument, MarkdownDocument, PLUGIN, is_epub,
    provider,
};
use paperpaw_sdk::document::Document;

/// The `core.format` provider of EPUB books (`epub`): sniffed by its
/// `mimetype` entry, the opened file named by the call's `text`.
#[must_use]
pub fn epub_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str());
    provider::FormatProvider::new(
        "format-epub.epub",
        |storage, name, options| {
            EpubDocument::open_options(storage, name, options)
                .map(|d| Box::new(d) as Box<dyn Document>)
        },
        dir,
        is_epub,
    )
}

/// The `core.format` provider of FictionBooks (`fb2`, `fb2.zip`): a
/// `siblings` format, the opened file named by the call's `text`.
#[must_use]
pub fn fb2_provider() -> provider::FormatProvider {
    let dir = paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str());
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
    let dir = paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str());
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
    let dir = paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str());
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
    let dir = paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str());
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
