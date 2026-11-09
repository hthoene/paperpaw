//! Typed library queries and book updates, with caller-owned reply buffers.
use super::wire::services_wire::core_library_v1 as schema;
use super::{
    Error, Host,
    declare::records_wire::{self as wire, RawRecord, WireEncode, WireRecord},
    services::Service,
};

mod facets;
pub use facets::{Facet, FacetPage, FacetRow};
mod annotations;
pub use annotations::{
    Annotation, AnnotationCursor, AnnotationKind, AnnotationPage, AnnotationPageEnd,
};
mod annotation_text;
pub use annotation_text::{AnnotationTextKind, AnnotationTextPart, AnnotationTextQuery};
mod fields;
pub use fields::{Field, FieldCursor, FieldPage, FieldPageEnd};
mod position;
pub use position::{BookKey, Position, PositionWrite};
mod query;
mod rows;
pub use query::{CatalogCursor, Query, Show, Sort};
pub use rows::{Book, CatalogPhase, CatalogState, Entry, LibraryPage, LibraryPageEnd};

/// A path or a generation-bound catalog handle identifying one book.
#[derive(Clone, Copy, Debug)]
pub struct BookRef<'a>(wire::ServiceLibRef<'a>);
impl<'a> BookRef<'a> {
    /// Identify a book by its complete card path.
    ///
    /// # Errors
    /// Invalid for empty paths or control characters; NoSpace above the wire cap.
    pub fn path(path: &'a str) -> Result<Self, Error> {
        if path.is_empty() || path.chars().any(char::is_control) {
            return Err(Error::Invalid);
        }
        if path.len() > wire::SERVICE_LIB_REF_PATH_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        Ok(Self(wire::ServiceLibRef {
            path,
            generation: 0,
            key: None,
        }))
    }
    /// Identify a catalog row in its original generation.
    ///
    /// # Errors
    /// Invalid for generation zero, which cannot identify a catalog snapshot.
    pub fn catalog(key: u32, generation: u32) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self(wire::ServiceLibRef {
            path: "",
            generation,
            key: Some(key),
        }))
    }
    fn record(&self) -> &dyn WireEncode {
        &self.0
    }
}

/// A library service handle. Release it explicitly within a callback.
#[derive(Debug)]
#[must_use = "release the library handle when finished"]
pub struct Library {
    service: Service,
}
impl Library {
    /// Open the metadata-declared library service.
    ///
    /// # Errors
    /// Returns the host's admission, availability or allocation refusal.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }
    /// Release this service handle using the current callback's host.
    ///
    /// # Errors
    /// Returns the host's stale-handle or ownership refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }
    fn call(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[&dyn WireEncode],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let len = wire::records_encode_counted_of(
            records.iter().copied(),
            usize::from(operation.input_rows),
            operation.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        self.service.call(host, op, &input[..len], out)
    }
    /// Read catalog state and its current generation.
    ///
    /// # Errors
    /// Returns host refusals or Invalid for a malformed reply.
    pub fn state(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<CatalogState, Error> {
        let n = self.call(host, schema::OP_STATE, &[], input, out)?;
        CatalogState::decode(one(&out[..n])?)
    }
    /// Enumerate books using the query and its generation-bound cursor.
    ///
    /// # Errors
    /// State for stale pages; Invalid for malformed replies; otherwise host errors.
    pub fn query<'a>(
        &self,
        host: &Host<'_>,
        query: &Query<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<LibraryPage<'a, Entry<'a>>, Error> {
        let n = self.call(host, schema::OP_QUERY, &[&query.record()], input, out)?;
        rows::page(
            &out[..n],
            Some(query.cursor),
            schema::PAGE_MAX_ROWS,
            Entry::decode,
        )
    }
    /// Read one book's metadata, including its full path.
    ///
    /// # Errors
    /// NotFound for an unknown book, State for a stale handle, or host/decode errors.
    pub fn book<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Book<'a>, Error> {
        let n = self.call(host, schema::OP_BOOK, &[book.record()], input, out)?;
        let row = Book::decode(one(&out[..n])?)?;
        if book.0.key.is_some_and(|key| key != row.key())
            || book.0.key.is_some() && book.0.generation != row.generation()
        {
            return Err(Error::State);
        }
        Ok(row)
    }
    /// Find the next catalogued book in the same series.
    ///
    /// # Errors
    /// NotFound for no next book; otherwise host/decode errors.
    pub fn next_in_series<'a>(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<Entry<'a>, Error> {
        let n = self.call(
            host,
            schema::OP_NEXT_IN_SERIES,
            &[book.record()],
            input,
            out,
        )?;
        Entry::decode(one(&out[..n])?)
    }
    /// Set or remove a full field id (`plugin.name`) owned by the caller.
    /// An empty value removes it; relative declaration names are not accepted.
    ///
    /// # Errors
    /// Returns host authority/refusal errors or Invalid for a nonempty reply.
    pub fn set_field(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        field: &str,
        value: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        super::ui::navigation::full_id(field)?;
        let row = wire::ServiceLibFieldSet { field, value };
        self.mutate(
            host,
            schema::OP_SET_FIELD,
            &[book.record(), &row],
            input,
            out,
        )
    }
    /// Mark a book finished or clear its finished status without changing other status bits.
    ///
    /// # Errors
    /// Returns host authority/refusal errors or Invalid for a nonempty reply.
    pub fn set_finished(
        &self,
        host: &Host<'_>,
        book: BookRef<'_>,
        finished: bool,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let row = wire::ServiceLibStatus {
            set: u32::from(finished),
            clear: u32::from(!finished),
        };
        self.mutate(
            host,
            schema::OP_SET_STATUS,
            &[book.record(), &row],
            input,
            out,
        )
    }
    fn mutate(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[&dyn WireEncode],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        if self.call(host, op, records, input, out)? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
fn one(bytes: &[u8]) -> Result<RawRecord<'_>, Error> {
    RawRecord::one(bytes).map_err(|_| Error::Invalid)
}

#[cfg(test)]
mod tests;
