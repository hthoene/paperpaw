//! REG page framing, independent of the live table visitor.
//! The caller visits ordered rows starting at the requested cursor. A stopped
//! row stays unconsumed for the next page. Nothing is retained between calls.
use super::registrations::{self as wire, Record};
use crate::abi7::context::registry_views::Row;
use crate::abi7::services_wire::core_registry_v1;
use crate::native_host::codec;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE};

pub(crate) struct RegistryPage<'a> {
    out: &'a mut [u8],
    used: usize,
    rows: usize,
    cursor: u32,
    generation: u32,
    total: u32,
    end_bytes: usize,
    /// Whole rows one page holds at most.
    max_rows: usize,
}

impl<'a> RegistryPage<'a> {
    pub(crate) fn new(
        query: &wire::ServiceRegQuery<'_>,
        generation: u32,
        total: u32,
        out: &'a mut [u8],
    ) -> Result<Self, i32> {
        if generation == 0
            || query.generation != 0 && query.generation != generation
            || query.generation == 0 && query.cursor != 0
        {
            return Err(ERR_STATE);
        }
        if query.cursor > total {
            return Err(ERR_INVALID);
        }
        let end_bytes = codec::framed_len(&end(total, generation, query.cursor))?;
        if out.len() < end_bytes {
            return Err(ERR_NO_SPACE);
        }
        Ok(Self {
            out,
            used: 0,
            rows: 0,
            cursor: query.cursor,
            generation,
            total,
            end_bytes,
            max_rows: core_registry_v1::PAGE_MAX_ROWS,
        })
    }

    /// A page of at most `rows` whole rows (a service whose reply schema says
    /// fewer than `core.registry`'s 32).
    pub(crate) fn rows_at_most(mut self, rows: usize) -> Self {
        self.max_rows = rows;
        self
    }

    /// Continue consumes the row; Break leaves it for the next page.
    pub(crate) fn push(&mut self, record: &dyn Row) -> Result<ControlFlow<()>, i32> {
        // A row past `total`: the tables changed between the counting walk
        // and this one.
        if self.cursor == self.total {
            return Err(ERR_STATE);
        }
        if self.rows == self.max_rows {
            return Ok(ControlFlow::Break(()));
        }
        // The room before the end record, which `new` saw to.
        let limit = self.out.len() - self.end_bytes;
        let room = self.out.get_mut(self.used..limit).ok_or(ERR_NO_SPACE)?;
        let Some(written) = record.encode_within(room)? else {
            return if self.rows == 0 {
                Err(ERR_NO_SPACE)
            } else {
                Ok(ControlFlow::Break(()))
            };
        };
        self.used += written;
        self.rows += 1;
        self.cursor += 1;
        Ok(ControlFlow::Continue(()))
    }

    pub(crate) fn finish(mut self) -> Result<usize, i32> {
        if self.cursor != self.total && self.rows == 0 {
            return Err(ERR_INVALID);
        }
        let record = end(self.total, self.generation, self.cursor);
        self.used += codec::encode_one(&record, &mut self.out[self.used..])?;
        Ok(self.used)
    }
}

fn end(total: u32, generation: u32, next: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags: u32::from(next == total),
        total,
        generation,
        next,
        cursor: None,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
    use super::*;

    fn query(cursor: u32, generation: u32) -> wire::ServiceRegQuery<'static> {
        wire::ServiceRegQuery {
            scope: "",
            cursor,
            generation,
            flags: 0,
        }
    }

    fn row() -> Record<'static> {
        Record::ServiceRegChoice(wire::ServiceRegChoice {
            value: "one",
            title: "One",
        })
    }

    fn page_end(mut bytes: &[u8]) -> (usize, wire::ServiceEnd) {
        let mut rows = 0;
        loop {
            let (record, length) = codec::decode_any_prefix(bytes).unwrap();
            bytes = &bytes[length..];
            if let Record::ServiceEnd(end) = record {
                assert!(bytes.is_empty());
                return (rows, end);
            }
            assert!(matches!(record, Record::ServiceRegChoice(_)));
            rows += 1;
        }
    }

    #[test]
    fn full_pages_keep_whole_rows_and_continue_the_same_generation() {
        let mut out = [0; 4096];
        let total = core_registry_v1::PAGE_MAX_ROWS as u32 + 3;
        let mut first = RegistryPage::new(&query(0, 0), 7, total, &mut out).unwrap();
        for _ in 0..core_registry_v1::PAGE_MAX_ROWS {
            assert_eq!(first.push(&row()).unwrap(), ControlFlow::Continue(()));
        }
        assert_eq!(first.push(&row()).unwrap(), ControlFlow::Break(()));
        let length = first.finish().unwrap();
        let (rows, next) = page_end(&out[..length]);
        assert_eq!(rows, core_registry_v1::PAGE_MAX_ROWS);
        assert_eq!((next.flags, next.generation, next.total), (0, 7, total));
        let mut last =
            RegistryPage::new(&query(next.next, next.generation), 7, total, &mut out).unwrap();
        for _ in 0..3 {
            assert_eq!(last.push(&row()).unwrap(), ControlFlow::Continue(()));
        }
        let length = last.finish().unwrap();
        let (rows, next) = page_end(&out[..length]);
        assert_eq!(
            (rows, next.flags, next.next, next.total),
            (3, 1, total, total)
        );
    }

    #[test]
    fn stale_or_malformed_cursors_leave_output_untouched() {
        let mut out = [77; 128];
        for (query, status) in [
            (query(1, 0), ERR_STATE),
            (query(0, 2), ERR_STATE),
            (query(3, 1), ERR_INVALID),
        ] {
            assert!(
                matches!(RegistryPage::new(&query, 1, 2, &mut out), Err(error) if error == status)
            );
            assert_eq!(out, [77; 128]);
        }
    }

    #[test]
    fn byte_capacity_reserves_the_end_and_never_splits_a_row() {
        let end_bytes = codec::framed_len(&end(2, 1, 1)).unwrap();
        let row_bytes = codec::framed_len(&row()).unwrap();
        let mut out = [0; 128];
        let mut page =
            RegistryPage::new(&query(0, 0), 1, 2, &mut out[..end_bytes + row_bytes]).unwrap();
        assert_eq!(page.push(&row()).unwrap(), ControlFlow::Continue(()));
        assert_eq!(page.push(&row()).unwrap(), ControlFlow::Break(()));
        let length = page.finish().unwrap();
        assert_eq!(page_end(&out[..length]).0, 1);
        let mut page = RegistryPage::new(&query(0, 0), 1, 1, &mut out[..end_bytes]).unwrap();
        assert_eq!(page.push(&row()), Err(ERR_NO_SPACE));
    }

    #[test]
    fn a_row_past_the_total_is_a_state_error() {
        let mut out = [0; 256];
        let mut page = RegistryPage::new(&query(0, 0), 1, 1, &mut out).unwrap();
        assert_eq!(page.push(&row()).unwrap(), ControlFlow::Continue(()));
        assert_eq!(page.push(&row()), Err(ERR_STATE));
    }

    #[test]
    fn empty_table_has_exactly_one_terminal_record() {
        let mut out = [0; 128];
        let page = RegistryPage::new(&query(0, 0), 3, 0, &mut out).unwrap();
        let length = page.finish().unwrap();
        let (rows, end) = page_end(&out[..length]);
        assert_eq!(
            (rows, end.flags, end.total, end.next, end.generation),
            (0, 1, 0, 0, 3)
        );
    }
}
