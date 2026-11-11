//! The commit of a staged transaction: every edit applies or none does. The
//! checks come first and write nothing; the rewrite then goes to a temporary
//! file and is renamed over the annotation file as its last step, so the old
//! file is whole at every power cut. See `lib_edits.rs` for the memory.
use super::{LibEdits, receipt, stage};
use crate::native_host::lib_service::{CommitRequest, Committed};
use crate::native_host::registrations as wire;
use alloc::{string::String, vec::Vec};
use core::cell::Cell;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};
use paperpaw_sdk::annotations::{
    Kind,
    write::{self, Edits, Existing, Spec},
};
use paperpaw_sdk::data::Error;
use paperpaw_sdk::document::Locator;

/// `lib_edit.action`.
const ADD: u8 = 0;
const REPLACE: u8 = 1;
const REMOVE: u8 = 2;
const UPSERT: u8 = 3;
/// `lib_edit.flags`.
const HAS_CONVERSION: u32 = 1 << 3;
const HAS_XPOINTERS: u32 = 1 << 4;
/// The longest a row's text may be: a row is one line of the file.
const LINE: usize = paperpaw_sdk::annotations::LINE_MAX;

/// What the commit keeps of an edit while it checks: 40 bytes.
#[derive(Clone, Copy)]
struct Item {
    action: u8,
    /// The annotation it changes, or the one a key matched; 0 for a new one.
    id: u32,
    start: (u32, u32),
    end: (u32, u32),
    /// The FNV-1a hash of its client key; the key itself is read again from
    /// the staged file when the hashes meet.
    key_hash: u32,
    keyed: bool,
    /// Where its record starts in the staged file.
    offset: u32,
}

fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The status of a writer error.
fn status(error: Error) -> i32 {
    match error {
        Error::OutOfMemory => ERR_NO_MEMORY,
        Error::Storage(error) => stage::status(error),
        Error::Unreadable => ERR_INVALID,
        _ => ERR_IO,
    }
}

fn table<T>(count: usize) -> Result<Vec<T>, i32> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
    Ok(rows)
}

pub(super) fn run(host: &LibEdits, request: &CommitRequest) -> Result<Committed, i32> {
    let storage = host.stage.storage();
    let file = write::file_of_key(request.book.0).map_err(status)?;
    let info = write::try_info(storage, &file).map_err(status)?;
    // The caller read its generation before it began: if the book moved on,
    // it reads again, merges and begins again.
    if info.generation != request.base_generation {
        return Err(ERR_STATE);
    }
    let mut items = collect(host, request)?;
    check(host, request, &file, &mut items)?;
    // The ids of the new rows, in the order of the edits.
    let mut next = info.next_id;
    let mut rows = table::<(u32, u32)>(items.len())?;
    for item in &mut items {
        let action = match item.action {
            ADD => 0,
            UPSERT if item.id == 0 => 0,
            UPSERT => 3,
            other => u32::from(other),
        };
        if action == 0 {
            item.id = next;
            next = next.checked_add(1).ok_or(ERR_NO_SPACE)?;
        }
        rows.push((item.id, action));
    }
    let added = next - info.next_id;
    let mut drops = table::<u32>(items.len())?;
    drops.extend(
        items
            .iter()
            .filter(|i| {
                matches!(i.action, REPLACE | REMOVE) || (i.action == UPSERT && i.id < info.next_id)
            })
            .map(|i| i.id),
    );
    drops.sort_unstable();
    let mut news = table::<u32>(items.len())?;
    news.extend((0..items.len() as u32).filter(|i| items[*i as usize].action != REMOVE));
    news.sort_by_key(|i| (items[*i as usize].start, items[*i as usize].end, *i));
    let generation = write::generation_after(&info);
    // The receipt is made before the rename: nothing may fail once the file
    // is changed, or the plugin would retry and add everything twice.
    receipt::write(
        host,
        request.transaction,
        (request.owner, generation),
        &rows,
    )?;
    let tmp = stage::path(request.transaction, "new")?;
    let mut merge = Merge {
        host,
        transaction: request.transaction,
        items: &items,
        drops: &drops,
        news: &news,
        next: 0,
        record: stage::buffer(stage::RECORD_MAX)
            .map_err(|_| ERR_NO_MEMORY)
            .ok(),
        failure: Cell::new(0),
    };
    let done = write::rewrite(storage, &file, &tmp, (&info, added), &mut merge);
    match done {
        Ok(written) => Ok(Committed {
            annotation_generation: written,
            edits: request.edits,
            added,
        }),
        Err(error) => {
            receipt::remove(host, request.transaction);
            Err(match merge.failure.get() {
                0 => status(error),
                failure => failure,
            })
        }
    }
}

/// The edits of the staged file, in order: what is needed to check them.
fn collect(host: &LibEdits, request: &CommitRequest) -> Result<Vec<Item>, i32> {
    let mut items = table::<Item>(request.edits as usize)?;
    host.stage.each(request.transaction, &mut |at, staged| {
        if let stage::Staged::Edit(edit) = staged {
            if items.len() >= request.edits as usize {
                return ControlFlow::Break(ERR_INVALID);
            }
            let Ok(offset) = u32::try_from(at) else {
                return ControlFlow::Break(ERR_NO_SPACE);
            };
            items.push(Item {
                action: u8::try_from(edit.action).unwrap_or(u8::MAX),
                id: edit.id,
                start: (edit.start_section, edit.start_offset),
                end: (edit.end_section, edit.end_offset),
                key_hash: fnv(edit.client_key),
                keyed: !edit.client_key.is_empty(),
                offset,
            });
        }
        ControlFlow::Continue(())
    })?;
    if items.len() != request.edits as usize {
        return Err(ERR_INVALID);
    }
    Ok(items)
}

/// Checks the edits against the file's rows: an id that is not there is
/// `ERR_NOT_FOUND`; an id or a client key named twice, or an add of a key
/// the book has, is `ERR_INVALID`. An upsert whose key is there takes the
/// row's id.
fn check(
    host: &LibEdits,
    request: &CommitRequest,
    file: &str,
    items: &mut [Item],
) -> Result<(), i32> {
    // (id, item) of every edit that names a row, sorted by id.
    let mut named = table::<(u32, u32)>(items.len())?;
    for (n, item) in items.iter().enumerate() {
        if matches!(item.action, REPLACE | REMOVE) {
            named.push((item.id, n as u32));
        }
    }
    named.sort_unstable();
    if named.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(ERR_INVALID);
    }
    // (hash, item) of every add and upsert that has a client key.
    let mut keyed = table::<(u32, u32)>(items.len())?;
    for (n, item) in items.iter().enumerate() {
        if item.keyed && matches!(item.action, ADD | UPSERT) {
            keyed.push((item.key_hash, n as u32));
        }
    }
    keyed.sort_unstable();
    for pair in keyed.windows(2) {
        if pair[0].0 == pair[1].0 && same_key(host, request, items, pair[0].1, pair[1].1)? {
            return Err(ERR_INVALID);
        }
    }
    let mut found = table::<bool>(named.len())?;
    found.resize(named.len(), false);
    let failure = Cell::new(0);
    let scan = write::each_existing(host.stage.storage(), file, &mut |row: &Existing<'_>| {
        if let Ok(at) = named.binary_search_by_key(&row.id, |(id, _)| *id) {
            found[at] = true;
        }
        if row.key.is_empty() {
            return Ok(());
        }
        let hash = fnv(row.key);
        let from = keyed.partition_point(|(h, _)| *h < hash);
        for &(_, n) in keyed[from..].iter().take_while(|(h, _)| *h == hash) {
            match key_is(host, request, items[n as usize], row.key) {
                Ok(true) if items[n as usize].action == UPSERT => items[n as usize].id = row.id,
                Ok(true) => {
                    failure.set(ERR_INVALID);
                    return Err(Error::Unreadable);
                }
                Ok(false) => {}
                Err(error) => {
                    failure.set(error);
                    return Err(Error::Unreadable);
                }
            }
        }
        Ok(())
    });
    if let Err(error) = scan {
        return Err(match failure.get() {
            0 => status(error),
            failure => failure,
        });
    }
    if found.iter().any(|found| !found) {
        return Err(ERR_NOT_FOUND);
    }
    Ok(())
}

/// The staged edit at `item`, decoded into `visit`.
fn with_edit<R>(
    host: &LibEdits,
    request: &CommitRequest,
    item: Item,
    visit: impl FnOnce(&wire::ServiceLibEdit<'_>) -> Result<R, i32>,
) -> Result<R, i32> {
    let file = stage::path(request.transaction, "stage")?;
    let mut chunk = stage::buffer(stage::RECORD_MAX)?;
    let read = stage::read_full(
        host.stage.storage(),
        &file,
        u64::from(item.offset),
        &mut chunk,
    )?;
    let (raw, _) = wire::RawRecord::prefix(&chunk[..read]).map_err(|_| ERR_INVALID)?;
    visit(&crate::native_host::codec::decode(raw)?)
}

fn key_is(host: &LibEdits, request: &CommitRequest, item: Item, key: &str) -> Result<bool, i32> {
    with_edit(host, request, item, |edit| Ok(edit.client_key == key))
}

fn same_key(
    host: &LibEdits,
    request: &CommitRequest,
    items: &[Item],
    a: u32,
    b: u32,
) -> Result<bool, i32> {
    let first = items[a as usize];
    let mut key = String::new();
    with_edit(host, request, first, |edit| {
        key.try_reserve_exact(edit.client_key.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        key.push_str(edit.client_key);
        Ok(())
    })?;
    key_is(host, request, items[b as usize], &key)
}

/// What the writer asks while it merges: which old rows go and the new rows,
/// read from the staged file one at a time.
struct Merge<'a> {
    host: &'a LibEdits,
    transaction: u32,
    items: &'a [Item],
    drops: &'a [u32],
    news: &'a [u32],
    next: usize,
    record: Option<Vec<u8>>,
    /// The status of a failure that is not the writer's.
    failure: Cell<i32>,
}

impl Merge<'_> {
    fn fail(&self, status: i32) -> Error {
        self.failure.set(status);
        Error::Unreadable
    }

    /// The row of the staged edit `index`, given to `write`.
    fn row(
        &mut self,
        index: usize,
        write: &mut dyn FnMut(&Spec<'_>) -> Result<(), Error>,
    ) -> Result<(), i32> {
        let item = self.items[index];
        let file = stage::path(self.transaction, "stage")?;
        let chunk = self.record.as_mut().ok_or(ERR_NO_MEMORY)?;
        let storage = self.host.stage.storage();
        let read = stage::read_full(storage, &file, u64::from(item.offset), chunk)?;
        let (raw, used) = wire::RawRecord::prefix(&chunk[..read]).map_err(|_| ERR_INVALID)?;
        let edit: wire::ServiceLibEdit<'_> = crate::native_host::codec::decode(raw)?;
        // The texts that arrived in parts: up to a line, never more.
        let mut parts = Parts::new(&edit)?;
        if parts.needed() {
            let mut at = u64::from(item.offset) + used as u64;
            let size = storage.size(&file).map_err(stage::status)?;
            let mut edits_after = 0;
            let mut part_chunk = stage::buffer(stage::RECORD_MAX)?;
            while at < size && edits_after < 3 {
                let read = stage::read_full(storage, &file, at, &mut part_chunk)?;
                let (raw, used) =
                    wire::RawRecord::prefix(&part_chunk[..read]).map_err(|_| ERR_INVALID)?;
                if raw.tag() == <wire::ServiceLibEdit<'_> as wire::WireRecord<'_>>::TAG {
                    edits_after += 1;
                } else {
                    let part: wire::ServiceLibEditText<'_> =
                        crate::native_host::codec::decode(raw)?;
                    if part.edit as usize == index {
                        parts.add(&part)?;
                    }
                }
                at += used as u64;
            }
        }
        let kind = match edit.kind {
            0 => Kind::Bookmark,
            1 => Kind::Highlight,
            other => Kind::Other(u16::try_from(other - 2).map_err(|_| ERR_INVALID)?),
        };
        let mut spec = Spec::new(
            item.id,
            kind,
            Locator::new(edit.start_section, edit.start_offset),
            Locator::new(edit.end_section, edit.end_offset),
        );
        spec.percent = u8::try_from(edit.percent).map_err(|_| ERR_INVALID)?;
        spec.created = i64::try_from(edit.created).unwrap_or(0);
        spec.updated = i64::try_from(edit.updated).unwrap_or(0);
        spec.conversion = (edit.flags & HAS_CONVERSION != 0).then_some(edit.conversion);
        spec.key = edit.client_key;
        spec.text = parts.text(edit.text);
        spec.note = parts.note(edit.note);
        if edit.flags & HAS_XPOINTERS != 0 {
            spec.start_xpointer = parts.xpointer(0);
            spec.end_xpointer = parts.xpointer(1);
        }
        write(&spec).map_err(|error| {
            self.failure.set(status(error.clone()));
            ERR_INVALID
        })
    }
}

impl Edits for Merge<'_> {
    fn keeps(&mut self, row: &Existing<'_>) -> Result<bool, Error> {
        Ok(self.drops.binary_search(&row.id).is_err())
    }

    fn next_new(
        &mut self,
        limit: Option<(Locator, Locator)>,
        write: &mut dyn FnMut(&Spec<'_>) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let Some(&index) = self.news.get(self.next) else {
            return Ok(false);
        };
        let item = self.items[index as usize];
        if let Some((start, end)) = limit
            && (item.start, item.end) > ((start.section, start.offset), (end.section, end.offset))
        {
            return Ok(false);
        }
        self.row(index as usize, write).map_err(|status| {
            if self.failure.get() == 0 {
                self.fail(status)
            } else {
                Error::Unreadable
            }
        })?;
        self.next += 1;
        Ok(true)
    }
}

/// The text, the note and the XPointers of one edit that came in parts.
struct Parts {
    texts: [String; 4],
    wanted: [bool; 4],
}

impl Parts {
    fn new(edit: &wire::ServiceLibEdit<'_>) -> Result<Self, i32> {
        let wanted = [
            edit.text_bytes > 0,
            edit.note_bytes > 0,
            edit.flags & HAS_XPOINTERS != 0,
            edit.flags & HAS_XPOINTERS != 0,
        ];
        let sizes = [
            edit.text_bytes as usize,
            edit.note_bytes as usize,
            1024,
            1024,
        ];
        let mut texts = [String::new(), String::new(), String::new(), String::new()];
        for ((text, wanted), size) in texts.iter_mut().zip(wanted).zip(sizes) {
            // A text longer than a line cannot be a row.
            if wanted && size > LINE {
                return Err(ERR_NO_SPACE);
            }
            if wanted {
                text.try_reserve_exact(size).map_err(|_| ERR_NO_MEMORY)?;
            }
        }
        Ok(Self { texts, wanted })
    }

    fn needed(&self) -> bool {
        self.wanted.iter().any(|w| *w)
    }

    fn add(&mut self, part: &wire::ServiceLibEditText<'_>) -> Result<(), i32> {
        let slot = self.texts.get_mut(part.field as usize).ok_or(ERR_INVALID)?;
        if slot.len() + part.data.len() > LINE {
            return Err(ERR_NO_SPACE);
        }
        slot.try_reserve(part.data.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        slot.push_str(part.data);
        Ok(())
    }

    fn text<'a>(&'a self, inline: &'a str) -> &'a str {
        if self.wanted[0] {
            &self.texts[0]
        } else {
            inline
        }
    }

    fn note<'a>(&'a self, inline: &'a str) -> &'a str {
        if self.wanted[1] {
            &self.texts[1]
        } else {
            inline
        }
    }

    fn xpointer(&self, which: usize) -> &str {
        &self.texts[2 + which]
    }
}
