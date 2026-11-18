//! A scripted `core.registry`: list ops paged the way the host pages them
//! (`registry_page.rs`: a cursor, a generation that must not change between
//! pages, whole rows only, an end record), and single ops answered by a closure.
use paperpaw_core::native_host::registrations::{
    RawRecord, Record, ServiceEnd, ServiceRegQuery, record_encode_one,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

/// The `core.registry` major the scripted service answers at.
pub const MAJOR: u16 = 1;
/// The rows of a page (`core_registry_v1::PAGE_MAX_ROWS`).
pub const PAGE_ROWS: usize = 32;

type Single = Box<dyn FnMut(&[u8]) -> Result<Vec<u8>, i32>>;

struct Inner {
    rows: BTreeMap<u32, Vec<Vec<u8>>>,
    single: BTreeMap<u32, Single>,
    generation: Cell<u32>,
    page_rows: Cell<usize>,
    max_reply: Cell<usize>,
    pages: Cell<usize>,
}

/// The scripted registry; clones share rows and generation with the service.
#[derive(Clone)]
pub struct Registry {
    inner: Rc<RefCell<Inner>>,
}

impl core::fmt::Debug for Registry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Registry").finish_non_exhaustive()
    }
}

impl Registry {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                rows: BTreeMap::new(),
                single: BTreeMap::new(),
                generation: Cell::new(1),
                page_rows: Cell::new(PAGE_ROWS),
                max_reply: Cell::new(4096),
                pages: Cell::new(0),
            })),
        }
    }

    /// The rows list op `op` answers: each is one bare framed record (build
    /// them with [`row`]). Changing them does not change the generation.
    pub fn set_rows(&self, op: u32, rows: Vec<Vec<u8>>) {
        self.inner.borrow_mut().rows.insert(op, rows);
    }

    /// Answers op `op` (a single-value op) with `handler(input)`.
    pub fn on(&self, op: u32, handler: impl FnMut(&[u8]) -> Result<Vec<u8>, i32> + 'static) {
        self.inner.borrow_mut().single.insert(op, Box::new(handler));
    }

    /// The registry generation (starts at 1); a page request with another one
    /// is `ERR_STATE`, as when the registry changed under a reader. The host
    /// never has generation 0: with 0 set, every list call is `ERR_STATE`, as
    /// the host's page framing answers.
    pub fn set_generation(&self, generation: u32) {
        self.inner.borrow().generation.set(generation);
    }

    /// Rows per page (1 to 32; 0 is 1, because a page always takes a row that
    /// fits) and the bytes of a reply (up to the op's bound; a row that never
    /// fits is `ERR_NO_SPACE`).
    pub fn page_limits(&self, rows: usize, max_reply: usize) {
        let inner = self.inner.borrow();
        inner.page_rows.set(rows.clamp(1, PAGE_ROWS));
        inner.max_reply.set(max_reply);
    }

    /// How many list pages were served.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.inner.borrow().pages.get()
    }

    pub(crate) fn call(&self, op: u32, input: &[u8]) -> Result<Vec<u8>, i32> {
        let mut inner = self.inner.borrow_mut();
        if let Some(handler) = inner.single.get_mut(&op) {
            return handler(input);
        }
        let query = RawRecord::single(input)?.decode::<ServiceRegQuery<'_>>()?;
        let rows = inner.rows.get(&op).ok_or(ERR_UNSUPPORTED)?;
        let generation = inner.generation.get();
        if generation == 0
            || query.generation != 0 && query.generation != generation
            || query.generation == 0 && query.cursor != 0
        {
            return Err(ERR_STATE);
        }
        let total = u32::try_from(rows.len()).map_err(|_| ERR_NO_SPACE)?;
        if query.cursor > total {
            return Err(ERR_INVALID);
        }
        let end_len = end(total, generation, query.cursor)?.len();
        let budget = inner.max_reply.get();
        if budget < end_len {
            return Err(ERR_NO_SPACE);
        }
        let mut out = Vec::new();
        let mut at = query.cursor;
        let mut taken = 0;
        while at < total && taken < inner.page_rows.get() {
            let row = &rows[at as usize];
            if out.len() + row.len() + end_len > budget {
                if taken == 0 {
                    return Err(ERR_NO_SPACE);
                }
                break;
            }
            out.extend_from_slice(row);
            at += 1;
            taken += 1;
        }
        out.extend_from_slice(&end(total, generation, at)?);
        inner.pages.set(inner.pages.get() + 1);
        Ok(out)
    }
}

fn end(total: u32, generation: u32, next: u32) -> Result<Vec<u8>, i32> {
    row(&Record::ServiceEnd(ServiceEnd {
        flags: u32::from(next == total),
        total,
        generation,
        next,
        cursor: None,
    }))
}

/// One bare framed record, as a list row or a single reply.
///
/// # Errors
/// The codec's status for a record it cannot encode.
pub fn row(record: &Record<'_>) -> Result<Vec<u8>, i32> {
    let mut bytes = vec![0_u8; 4096];
    let len = record_encode_one(record, &mut bytes)?;
    bytes.truncate(len);
    Ok(bytes)
}
