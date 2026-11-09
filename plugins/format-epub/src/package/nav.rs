//! The table of contents of a package: the EPUB 3 navigation document and
//! the EPUB 2 NCX, read into entries and printed pages.

use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::xml::{Token, decode, local_name};
use paperpaw_sdk::zip::ZipArchive;

use super::{capture, clean, dir_of, keep, tokenize, try_resolve};
use crate::fallible;

pub(super) type AddToc<'a> = dyn FnMut(String, u8, &str, Option<&str>) + 'a;

/// EPUB 3: `<nav epub:type="toc">` with nested `<ol><li><a href>`, and
/// the printed pages of `<nav epub:type="page-list">` for `page`.
pub(super) fn read_nav(
    storage: &dyn Storage,
    archive: &ZipArchive,
    path: &str,
    add: &mut AddToc<'_>,
    page: &mut dyn FnMut(String, &str, Option<&str>),
) -> Result<(), DocumentError> {
    let base = fallible::string(dir_of(path))?;
    let mut in_toc = false;
    let mut in_pages = false;
    let mut page_link: Option<(String, Option<String>)> = None;
    let mut ol_depth = 0u8;
    let mut link: Option<(String, Option<String>)> = None;
    let mut text = String::new();
    let mut out_of_memory = false;
    tokenize(storage, archive, path, &mut |token| match token {
        Token::Start {
            name,
            attrs,
            self_closing,
        } => match local_name(name) {
            "nav" => {
                let kind = attrs
                    .iter()
                    .find(|(n, _)| local_name(n) == "type")
                    .map(|(_, v)| v);
                if kind.is_some_and(|k| k.split_whitespace().any(|t| t == "toc")) {
                    in_toc = true;
                    ol_depth = 0;
                }
                in_pages = kind.is_some_and(|k| k.split_whitespace().any(|t| t == "page-list"));
            }
            "a" if in_pages && !self_closing => {
                let href = attrs.get("href").unwrap_or_default();
                page_link = keep(&mut out_of_memory, try_resolve(&base, &href));
                text.clear();
            }
            "ol" if in_toc => ol_depth = ol_depth.saturating_add(1),
            "a" if in_toc && !self_closing => {
                let href = attrs.get("href").unwrap_or_default();
                link = keep(&mut out_of_memory, try_resolve(&base, &href));
                text.clear();
            }
            _ => {}
        },
        Token::Text(t) if link.is_some() || page_link.is_some() => {
            out_of_memory |= !capture(&mut text, &decode(&t));
        }
        Token::End { name } => match local_name(name) {
            "nav" => {
                in_toc = false;
                in_pages = false;
            }
            "a" if page_link.is_some() => {
                if let Some((file, fragment)) = page_link.take() {
                    let Some(label) = keep(&mut out_of_memory, clean(&text)) else {
                        return;
                    };
                    if !label.is_empty() && !file.is_empty() {
                        page(label, &file, fragment.as_deref());
                    }
                }
            }
            "ol" if in_toc => ol_depth = ol_depth.saturating_sub(1),
            "a" => {
                if let Some((file, fragment)) = link.take() {
                    let Some(title) = keep(&mut out_of_memory, clean(&text)) else {
                        return;
                    };
                    if !title.is_empty() && !file.is_empty() {
                        add(
                            title,
                            ol_depth.saturating_sub(1),
                            &file,
                            fragment.as_deref(),
                        );
                    }
                }
            }
            _ => {}
        },
        _ => {}
    })?;
    if out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    Ok(())
}

/// EPUB 2: nested `<navPoint>` with `<navLabel><text>` and `<content src>`
/// inside `<navMap>` (page lists and other lists are not chapters).
pub(super) fn read_ncx(
    storage: &dyn Storage,
    archive: &ZipArchive,
    path: &str,
    add: &mut AddToc<'_>,
) -> Result<(), DocumentError> {
    let base = fallible::string(dir_of(path))?;
    let mut depth = 0u8;
    let mut in_map = false;
    let mut in_label = false;
    let mut text = String::new();
    let mut pending: Option<(String, u8)> = None;
    let mut out_of_memory = false;
    tokenize(storage, archive, path, &mut |token| match token {
        Token::Start { name, attrs, .. } => match local_name(name) {
            "navMap" => in_map = true,
            _ if !in_map => {}
            "navPoint" => depth = depth.saturating_add(1),
            "navLabel" => {
                in_label = true;
                text.clear();
            }
            "content" => {
                if let (Some((title, level)), Some(src)) = (pending.take(), attrs.get("src"))
                    && let Some((file, fragment)) =
                        keep(&mut out_of_memory, try_resolve(&base, &src))
                {
                    add(title, level, &file, fragment.as_deref());
                }
            }
            _ => {}
        },
        Token::Text(t) if in_label => out_of_memory |= !capture(&mut text, &decode(&t)),
        Token::End { name } => match local_name(name) {
            "navMap" => in_map = false,
            "navPoint" => depth = depth.saturating_sub(1),
            "navLabel" if in_map => {
                in_label = false;
                if let Some(title) = keep(&mut out_of_memory, clean(&text))
                    && !title.is_empty()
                {
                    pending = Some((title, depth.saturating_sub(1)));
                }
            }
            _ => {}
        },
        _ => {}
    })?;
    if out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    Ok(())
}
