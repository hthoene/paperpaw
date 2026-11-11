//! Op 22 `fields` over the card (`docs/abi/library.md` 4): the fields of every
//! plugin kept with a book, ordered by the owning plugin's id and then the
//! field's name, paged by cursor under the book's field generation.
//!
//! The values are read from the book's details (`paperpaw_sdk::library::
//! try_book`, the one reader; a book's details are 16 KiB at most, and so is
//! what a call holds, once); no list outlives the call. The field generation
//! is a hash of the book's fields (their ids and values, in order), never 0,
//! so it changes with every change of them: a `set_field` that changes a
//! value, a removal, a move of the data that changes a field. (A `set_field`
//! that writes the value a field already has changes nothing a page could
//! show, and so does not advance it.)
use super::query::head;
use super::{CatalogReads, fail};
use crate::abi7::context::RowsEnd;
use crate::native_host::lib_service::{BookRef, Rows};
use crate::native_host::registrations::{self as wire, Record};
use alloc::vec::Vec;
use core::cmp::Ordering;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::digest::Fnv1a64;
use paperpaw_sdk::library;

/// Bytes of a `lib_field` value and of its id and title.
const VALUE: usize = 1024;
const ID: usize = 128;
const TITLE: usize = 128;

/// `<plugin>.<name>` as it is ordered: by the plugin's id, then the name,
/// each in byte order (a plugin id has no dot).
fn order(a: &str, b: &str) -> Ordering {
    fn split(id: &str) -> (&str, &str) {
        id.split_once('.').unwrap_or((id, ""))
    }
    split(a).cmp(&split(b))
}

/// A field id the wire can carry: `<plugin>.<name>` of at most 128 bytes.
fn listed(id: &str) -> bool {
    id.len() <= ID && id.contains('.') && !id.starts_with('.') && !id.ends_with('.')
}

impl CatalogReads {
    /// Op 22: the rows from `cursor` under the book's field generation.
    pub(crate) fn read_fields(
        &self,
        book: BookRef<'_>,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.with_path(book, |path| {
            let held = library::try_book(&*self.storage, path)
                .map_err(fail)?
                .ok_or(ERR_NOT_FOUND)?;
            // The fields of the wire, by their places: indexes, put in order.
            let mut places: Vec<usize> = Vec::new();
            places
                .try_reserve_exact(held.fields.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            for (at, (id, _)) in held.fields.iter().enumerate() {
                if listed(id) {
                    places.push(at);
                }
            }
            // Insertion: a book holds up to 64 fields, and no sort of the
            // standard library (its frames) is wanted here.
            for i in 1..places.len() {
                let mut j = i;
                while j > 0
                    && order(&held.fields[places[j - 1]].0, &held.fields[places[j]].0).is_gt()
                {
                    places.swap(j - 1, j);
                    j -= 1;
                }
            }
            let mut hash = Fnv1a64::new();
            for &at in &places {
                let (id, value) = &held.fields[at];
                for part in [id.as_bytes(), &[0], value.as_bytes(), &[0]] {
                    hash.feed(part);
                }
            }
            let digest = hash.value();
            let generation_now = u32::try_from((digest ^ (digest >> 32)) & 0xffff_ffff)
                .unwrap_or(1)
                .max(1);
            if generation != 0 && generation != generation_now {
                return Err(ERR_STATE);
            }
            let total = u32::try_from(places.len()).unwrap_or(u32::MAX);
            let start = cursor.min(total);
            let mut rows = 0_u32;
            for &at in places.iter().skip(start as usize) {
                let (id, value) = &held.fields[at];
                let mut title = [0_u8; TITLE];
                let mut length = None;
                self.env.field_title(id, &mut |text| {
                    let text = head(text, TITLE);
                    title[..text.len()].copy_from_slice(text.as_bytes());
                    length = Some(text.len());
                });
                let name = id.split_once('.').map_or("", |(_, name)| name);
                let shown = visit(Record::ServiceLibField(wire::ServiceLibField {
                    id,
                    title: length.map_or(head(name, TITLE), |n| {
                        core::str::from_utf8(&title[..n]).unwrap_or_default()
                    }),
                    value: head(value, VALUE),
                }));
                if shown.is_break() {
                    break;
                }
                rows += 1;
            }
            Ok(RowsEnd {
                total,
                generation: generation_now,
                next: start + rows,
            })
        })
    }
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
