//! Reading the sorted books as a view does: through once to count them,
//! then a page at a time.
//!
//! A view lists the books read last first (`by_recent`) or all by title.
//! Stacked, the books of a series show as one stack where the first of
//! them would (as [`Index::stacked`](crate::library::index::Index::stacked)
//! does): at the place of the book of it read last, else at its first by
//! title; books being read stay apart.

use alloc::vec::Vec;

use super::{
    BLOCK, CHECK_START, CHUNK, Counts, Entry, MAX_APART, MAX_NEXT, RECORD, Rec, Recent, SHOWN,
    SHOWS, Sorted, View, checked, show_index,
};
use crate::Storage;
use crate::library::catalog::{self, Show};
use crate::library::index::{self, FAVOURITE, Item, NEVER, NO_SERIES, NONE, OPENED};

/// Being read: told apart from its series' stack, as the index does.
fn being_read(item: &Item) -> bool {
    (1..100).contains(&item.progress)
}

/// Reads the books of the file at `path` from book `from` to `to`, `f`
/// called with each and its place until it says stop; `Some(check)` of
/// the bytes read where all were read whole.
fn each_rec(
    storage: &dyn Storage,
    path: &str,
    from: usize,
    to: usize,
    f: &mut dyn FnMut(usize, &Rec) -> bool,
) -> Option<u32> {
    let mut buf = [0u8; CHUNK * RECORD];
    let mut sum = CHECK_START;
    let mut at = from;
    while at < to {
        let n = (to - at).min(CHUNK);
        let want = &mut buf[..n * RECORD];
        let offset = (at * RECORD) as u64;
        if storage.read_at(path, offset, want).ok()? != want.len() {
            return None;
        }
        sum = checked(sum, want);
        for (i, bytes) in want.as_chunks::<RECORD>().0.iter().enumerate() {
            if !f(at + i, &Rec::decoded(bytes)) {
                return Some(sum);
            }
        }
        at += n;
    }
    Some(sum)
}

/// Whether the book of `rec` belongs to a stack (a series of books
/// counted).
fn in_series(rec: &Rec) -> bool {
    rec.series != 0 && rec.members >= 2
}

impl Sorted {
    /// The book of `rec` as things are now: how far read, favourite and
    /// opened as the log says, its place among the books read last;
    /// `None` where it was removed or is hidden.
    fn as_now(&self, rec: &Rec) -> Option<Item> {
        if self.hidden.contains(&rec.hash) {
            return None;
        }
        let (mut progress, mut flags) = (rec.progress, rec.flags);
        match self.changes.get_short(rec.hash) {
            Some(Some(status)) => {
                progress = status.progress.unwrap_or(NONE);
                flags =
                    (u8::from(status.favourite) * FAVOURITE) | (u8::from(status.opened) * OPENED);
            }
            Some(None) => return None,
            None => {}
        }
        let recent = self
            .opened
            .iter()
            .position(|&o| o == rec.hash)
            .map_or(NEVER, |r| r.min(usize::from(NEVER) - 1) as u8);
        Some(Item {
            offset: rec.offset,
            hash: rec.hash,
            key: rec.key,
            series: NO_SERIES,
            number: rec.number,
            progress,
            recent,
            flags,
        })
    }

    /// Books of `series` told apart before `pos` (all where `pos` is
    /// past the end).
    fn apart_before(&self, series: u32, pos: usize) -> usize {
        let pos = u32::try_from(pos).unwrap_or(u32::MAX);
        let start = self.apart.partition_point(|&(s, _)| s < series);
        let end = self.apart.partition_point(|&(s, p)| (s, p) < (series, pos));
        end - start
    }

    /// Whether the book of `series` at `pos` is in its series' stack.
    fn stacked_in(&self, series: u32, members: u16, pos: usize) -> bool {
        series != 0 && members >= 2 && self.apart.binary_search(&(series, pos as u32)).is_err()
    }

    /// How many books the stack of `series` (of `members`) holds.
    fn stack_of(&self, series: u32, members: u16) -> u16 {
        let apart = self.apart_before(series, usize::MAX);
        let held = usize::from(members).saturating_sub(apart).max(1);
        u16::try_from(held).unwrap_or(u16::MAX)
    }

    /// Whether the book of `rec` at `pos` shows in a stacked view by
    /// title: on its own, or heading its stack.
    fn heads_by_title(&self, rec: &Rec, pos: usize) -> bool {
        !self.stacked_in(rec.series, rec.members, pos)
            || usize::from(rec.rank) == self.apart_before(rec.series, pos)
    }

    /// The place among the books read last of the one heading the stack
    /// of `series`, if one does.
    fn head_rank(&self, series: u32) -> Option<u8> {
        self.heads
            .binary_search_by_key(&series, |&(s, _)| s)
            .ok()
            .map(|at| self.heads[at].1)
    }

    /// Reads the file through: each block's counts, the books read last,
    /// the books told apart; `None` where it is cut short or damaged.
    pub(super) fn read_through(
        &mut self,
        storage: &dyn Storage,
        records: Option<u32>,
    ) -> Option<()> {
        self.changes = catalog::changes(storage);
        self.opened = index::opened(storage);
        let mut blocks: Vec<Counts> = Vec::new();
        blocks.try_reserve_exact(self.count.div_ceil(BLOCK)).ok()?;
        blocks.resize(self.count.div_ceil(BLOCK), Counts::default());
        (self.recent, self.apart, self.heads) = (Vec::new(), Vec::new(), Vec::new());
        let mut recent: Vec<(u8, Recent)> = Vec::new();
        let mut apart: Vec<(u32, u32)> = Vec::new();
        // How many books apart of each series with some were met.
        let mut met: Vec<(u32, u16)> = Vec::new();
        let sum = each_rec(storage, &self.path, 0, self.count, &mut |pos, rec| {
            let Some(counts) = blocks.get_mut(pos / BLOCK) else {
                return false;
            };
            let item = self.as_now(rec);
            let alone = item.as_ref().is_none_or(being_read);
            let at = met.binary_search_by_key(&rec.series, |&(s, _)| s);
            let before = at.map_or(0, |at| met[at].1);
            let mut apart_now = false;
            if in_series(rec) && alone && apart.len() < MAX_APART {
                apart.push((rec.series, pos as u32));
                apart_now = true;
                match at {
                    Ok(at) => met[at].1 = met[at].1.saturating_add(1),
                    Err(at) => met.insert(at, (rec.series, 1)),
                }
            }
            let Some(item) = item else {
                return true;
            };
            let is_recent = item.recent != NEVER;
            if is_recent {
                let held = Recent {
                    pos: pos as u32,
                    item,
                    series: rec.series,
                    members: rec.members,
                };
                recent.push((item.recent, held));
            }
            for (s, show) in SHOWN.iter().enumerate() {
                if !is_recent && item.shown(*show) {
                    counts.shown[s] += 1;
                }
            }
            let heads = !in_series(rec) || apart_now || rec.rank == before;
            if heads && item.shown(Show::All) {
                counts.stacked[0] += 1;
                counts.stacked[1] += u16::from(!is_recent);
            }
            true
        })?;
        if records.is_some_and(|r| r != sum) {
            return None;
        }
        super::sort_by(&mut recent, |(rank, r)| {
            (u128::from(*rank) << 32) | u128::from(r.pos)
        });
        paperpaw_sort::sort_unstable(&mut apart);
        self.recent = recent.into_iter().map(|(_, r)| r).collect();
        self.apart = apart;
        self.count_heads(storage, &mut blocks)?;
        self.blocks = blocks;
        Some(())
    }

    /// The series a book read last heads, and their books by title left
    /// out of the stacked view with the books read last first.
    fn count_heads(&mut self, storage: &dyn Storage, blocks: &mut [Counts]) -> Option<()> {
        let mut heads: Vec<(u32, u8)> = Vec::new();
        for r in &self.recent {
            if !self.stacked_in(r.series, r.members, r.pos as usize) || !r.item.shown(Show::All) {
                continue;
            }
            if let Err(at) = heads.binary_search_by_key(&r.series, |&(s, _)| s) {
                // The first met is the one read last.
                heads.insert(at, (r.series, r.item.recent));
            }
        }
        self.heads = heads;
        if self.heads.is_empty() {
            return Some(());
        }
        each_rec(storage, &self.path, 0, self.count, &mut |pos, rec| {
            if self.head_rank(rec.series).is_none()
                || !self.stacked_in(rec.series, rec.members, pos)
            {
                return true;
            }
            let shown = self
                .as_now(rec)
                .is_some_and(|i| i.recent == NEVER && i.shown(Show::All));
            if shown
                && self.heads_by_title(rec, pos)
                && let Some(counts) = blocks.get_mut(pos / BLOCK)
            {
                counts.stacked[1] = counts.stacked[1].saturating_sub(1);
            }
            true
        })?;
        Some(())
    }

    /// The books read last `view` lists first, in their order.
    fn recent_entries(&self, view: View) -> impl Iterator<Item = Entry> + '_ {
        self.recent.iter().filter_map(move |r| {
            if !view.stacked {
                return r.item.shown(view.show).then_some(Entry {
                    item: r.item,
                    stack: 1,
                });
            }
            if !r.item.shown(Show::All) {
                return None;
            }
            if !self.stacked_in(r.series, r.members, r.pos as usize) {
                return Some(Entry {
                    item: r.item,
                    stack: 1,
                });
            }
            (self.head_rank(r.series) == Some(r.item.recent)).then(|| Entry {
                item: r.item,
                stack: self.stack_of(r.series, r.members),
            })
        })
    }

    /// How many books of block `b` the part by title of `view` shows.
    fn block_count(&self, view: View, b: usize) -> usize {
        let Some(counts) = self.blocks.get(b) else {
            return 0;
        };
        if view.stacked {
            return usize::from(counts.stacked[usize::from(view.by_recent)]);
        }
        let s = show_index(view.show).min(SHOWS - 1);
        let mut n = usize::from(counts.shown[s]);
        if !view.by_recent {
            // The books read last are in their places by title too.
            n += self
                .recent
                .iter()
                .filter(|r| r.pos as usize / BLOCK == b && r.item.shown(view.show))
                .count();
        }
        n
    }

    /// The entry of the book of `rec` at `pos` in the part by title of
    /// `view`, if it shows there.
    fn title_entry(&self, view: View, pos: usize, rec: &Rec) -> Option<Entry> {
        let item = self.as_now(rec)?;
        if view.by_recent && item.recent != NEVER {
            return None;
        }
        if !view.stacked {
            return item.shown(view.show).then_some(Entry { item, stack: 1 });
        }
        if !item.shown(Show::All) || !self.heads_by_title(rec, pos) {
            return None;
        }
        if !self.stacked_in(rec.series, rec.members, pos) {
            return Some(Entry { item, stack: 1 });
        }
        if view.by_recent && self.head_rank(rec.series).is_some() {
            return None;
        }
        Some(Entry {
            item,
            stack: self.stack_of(rec.series, rec.members),
        })
    }

    /// How many books `view` lists.
    pub fn len(&self, view: View) -> usize {
        let title: usize = (0..self.blocks.len())
            .map(|b| self.block_count(view, b))
            .sum();
        if view.by_recent {
            title + self.recent_entries(view).count()
        } else {
            title
        }
    }

    /// The books `view` lists from its `from`-th on, `n` at most.
    pub fn page(&self, storage: &dyn Storage, view: View, from: usize, n: usize) -> Vec<Entry> {
        let mut page = Vec::new();
        let mut from = from;
        if view.by_recent {
            let mut first = 0;
            for entry in self.recent_entries(view) {
                if first >= from && page.len() < n {
                    page.push(entry);
                }
                first += 1;
            }
            from = from.saturating_sub(first);
        }
        if page.len() >= n {
            return page;
        }
        // The block it starts in, and how many of it are before.
        let (mut skip, mut block) = (from, 0);
        while block < self.blocks.len() {
            let count = self.block_count(view, block);
            if skip < count {
                break;
            }
            skip -= count;
            block += 1;
        }
        each_rec(
            storage,
            &self.path,
            block * BLOCK,
            self.count,
            &mut |pos, rec| {
                if let Some(entry) = self.title_entry(view, pos, rec) {
                    if skip > 0 {
                        skip -= 1;
                    } else {
                        page.push(entry);
                    }
                }
                page.len() < n
            },
        );
        page
    }

    /// Where `view` lists the book hashing to `hash`; `None` where it does
    /// not.
    pub fn find(&self, storage: &dyn Storage, view: View, hash: u32) -> Option<usize> {
        let mut first = 0;
        if view.by_recent {
            for entry in self.recent_entries(view) {
                if entry.item.hash == hash {
                    return Some(first);
                }
                first += 1;
            }
        }
        let mut at = None;
        each_rec(storage, &self.path, 0, self.count, &mut |pos, rec| {
            if rec.hash == hash {
                at = Some((pos, *rec));
                return false;
            }
            true
        })?;
        let (pos, rec) = at?;
        self.title_entry(view, pos, &rec)?;
        let block = pos / BLOCK;
        let mut index: usize = (0..block).map(|b| self.block_count(view, b)).sum();
        each_rec(storage, &self.path, block * BLOCK, pos, &mut |p, r| {
            index += usize::from(self.title_entry(view, p, r).is_some());
            true
        })?;
        Some(first + index)
    }

    /// For each series begun (a book of it opened), its unread book of
    /// the lowest number, the series read last first (`by_recent`), else
    /// by title; `MAX_NEXT` series at most.
    pub fn next_in_series(&self, storage: &dyn Storage, by_recent: bool) -> Vec<Entry> {
        // The series begun, each with the place its book read last has.
        let mut begun: Vec<(u32, u8)> = Vec::new();
        each_rec(storage, &self.path, 0, self.count, &mut |_, rec| {
            let Some(item) = self.as_now(rec).filter(|_| rec.series != 0) else {
                return true;
            };
            if item.shown(Show::Unread) {
                return true;
            }
            match begun.binary_search_by_key(&rec.series, |&(s, _)| s) {
                Ok(at) => begun[at].1 = begun[at].1.min(item.recent),
                Err(at) if begun.len() < MAX_NEXT => begun.insert(at, (rec.series, item.recent)),
                Err(_) => {}
            }
            true
        });
        let mut next: Vec<Option<Item>> = alloc::vec![None; begun.len()];
        each_rec(storage, &self.path, 0, self.count, &mut |_, rec| {
            let Ok(at) = begun.binary_search_by_key(&rec.series, |&(s, _)| s) else {
                return true;
            };
            let Some(item) = self.as_now(rec).filter(|i| i.shown(Show::Unread)) else {
                return true;
            };
            if next[at].is_none_or(|n| (item.number, item.key) < (n.number, n.key)) {
                next[at] = Some(item);
            }
            true
        });
        let mut order: Vec<(u8, Item)> = begun
            .iter()
            .zip(next)
            .filter_map(|(&(_, recent), item)| {
                Some((if by_recent { recent } else { NEVER }, item?))
            })
            .collect();
        super::sort_by(&mut order, |(recent, item)| {
            super::ranked(*recent, item.key, item.offset)
        });
        order
            .into_iter()
            .map(|(_, item)| Entry { item, stack: 1 })
            .collect()
    }
}
