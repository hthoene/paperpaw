//! The index of the library's plugins, kept on the card (slice 4, host-free):
//! a line each (`index.txt`), where each line begins (`index.bin`, 4 bytes
//! each), the plugins a list picks for a start (`sections.txt`) and a head
//! (`index.head`) that says the rest; in memory only the runs of plugins of one
//! group. The list may grow without limit. The interface-6 index of
//! `catalog.rs` with the same files and format (`store-index 5`), so that an
//! index kept by Beta 3 is read as it is; made fallible: memory refused is
//! `NoMemory`, a card that does not answer is "no index", and nothing indexes
//! text or bytes in a way that can panic.
//!
//! The names are those of the plugin's data folder (`Root::Data`); the bytes
//! go through [`Storage`], which `DataStorage` is and a test's memory is.
use crate::native_item::Device;
use crate::native_list::{Hooks, List, ListReader};
use crate::native_model::{Entry, GROUPS, MAX_LINE, SECTIONS, add_to_run};
use crate::native_sort::{Cursor, KEYED, Keyed, Output, RUN, WRITE_AT, before, order_of, put};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::{buffer, lowercase, push},
    text::{concat, copy, write_to},
};

/// The files of the index and of a search.
pub const INDEX: &str = "index.txt";
pub const HEAD: &str = "index.head";
pub const OFFSETS: &str = "index.bin";
pub const SECTIONS_FILE: &str = "sections.txt";
pub const HITS: &str = "search.bin";
const SORT: &str = "sort.bin";
/// The first of an index's head: its format, then the runs of plugins of one
/// group (`group:count`), in order, then `fair` where the list is in a fair
/// order, then how many plugins each section has.
pub const HEADER: &str = "store-index 5";
/// The largest list the store takes from the library.
pub const MAX_LIST_FILE: u64 = 4 * 1024 * 1024;
/// A list is read in pieces of this many bytes.
const PIECE: usize = 1024;

/// The names of the files an index is made of, in the order index, offsets,
/// head, sections.
pub type Names<'a> = [&'a str; 4];

/// The line at `offset` of the file at `path` (a line past [`MAX_LINE`] is not
/// one); `None` where there is none or the card does not answer.
///
/// # Errors
/// `NoMemory`.
#[inline(never)]
pub fn line_at(storage: &dyn Storage, path: &str, offset: u32) -> Result<Option<String>, Error> {
    let mut buf = [0u8; 256];
    let mut at = u64::from(offset);
    let mut line = Vec::new();
    loop {
        let Ok(n) = storage.read_at(path, at, &mut buf) else {
            return Ok(None);
        };
        let Some(read) = buf.get(..n) else {
            return Ok(None);
        };
        if n == 0 {
            break;
        }
        if let Some(end) = read.iter().position(|&b| b == b'\n') {
            put(&mut line, read.get(..end).unwrap_or_default())?;
            break;
        }
        put(&mut line, read)?;
        if line.len() > MAX_LINE {
            return Ok(None);
        }
        at += n as u64;
    }
    Ok(String::from_utf8(line).ok())
}

/// The `n`th number of 4 bytes in the file at `path`.
fn number_at(storage: &dyn Storage, path: &str, n: usize) -> Option<u32> {
    let mut bytes = [0u8; 4];
    let read = storage.read_at(path, (n as u64) * 4, &mut bytes).ok()?;
    (read == 4).then(|| u32::from_le_bytes(bytes))
}

/// What is written of an index while a list is read.
struct Writer<'a> {
    storage: &'a dyn Storage,
    index: &'a str,
    offsets: &'a str,
    runs: Vec<(usize, u32)>,
    lines: Vec<u8>,
    starts: Vec<u8>,
    offset: u32,
    written: bool,
}

impl Writer<'_> {
    fn flush(&mut self) {
        self.written &= self.storage.append(self.index, &self.lines).is_ok();
        self.written &= self.storage.append(self.offsets, &self.starts).is_ok();
        self.lines.clear();
        self.starts.clear();
    }

    fn entry(&mut self, entry: &Entry) -> Result<(), Error> {
        add_to_run(&mut self.runs, entry.group)?;
        let line = entry.line()?;
        put(&mut self.starts, &self.offset.to_le_bytes())?;
        self.offset = self
            .offset
            .saturating_add(u32::try_from(line.len()).unwrap_or(u32::MAX));
        put(&mut self.lines, line.as_bytes())?;
        if self.lines.len() > WRITE_AT {
            self.flush();
        }
        Ok(())
    }
}

/// Writes the index of the list in the file `json` for `device` to the files
/// `names`. `None` (and no index) if it is no list this plugin reads, or too
/// big a file; the revocations it names and the sections come with a list.
///
/// # Errors
/// `NoMemory`; `Io` where the card refused a write (no index is left).
#[inline(never)]
pub fn build(
    storage: &dyn Storage,
    json: &str,
    names: Names<'_>,
    device: Device<'_>,
    hooks: Hooks<'_>,
) -> Result<Option<List>, Error> {
    let [index, offsets, head, sections] = names;
    match storage.size(json) {
        Ok(size) if size <= MAX_LIST_FILE => {}
        _ => return Ok(None),
    }
    let mut out = Writer {
        storage,
        index,
        offsets,
        runs: Vec::new(),
        lines: Vec::new(),
        starts: Vec::new(),
        offset: 0,
        written: storage.write(index, b"").is_ok() && storage.write(offsets, b"").is_ok(),
    };
    let read = read_json(storage, json, device, hooks, &mut out);
    let list = match read {
        Ok(list) => list,
        Err(error) => {
            remove_all(storage, names);
            return Err(error);
        }
    };
    out.flush();
    let Some(list) = list else {
        remove_all(storage, names);
        return Ok(None);
    };
    let ready = write_sections(storage, sections, &list)
        .and_then(|()| write_head(storage, head, &out.runs, &list));
    match ready {
        Ok(()) if out.written => Ok(Some(list)),
        Ok(()) => {
            remove_all(storage, names);
            Err(Error::Io)
        }
        Err(error) => {
            remove_all(storage, names);
            Err(error)
        }
    }
}

fn read_json(
    storage: &dyn Storage,
    json: &str,
    device: Device<'_>,
    hooks: Hooks<'_>,
    out: &mut Writer<'_>,
) -> Result<Option<List>, Error> {
    let mut reader = ListReader::new(device, hooks);
    let mut piece = buffer(PIECE)?;
    let mut at = 0u64;
    loop {
        let n = storage
            .read_at(json, at, &mut piece)
            .map_err(|_| Error::Io)?;
        if n == 0 {
            break;
        }
        reader.feed(piece.get(..n).unwrap_or_default(), &mut |e| out.entry(&e))?;
        at += n as u64;
    }
    reader.finish(&mut |e| out.entry(&e))
}

fn remove_all(storage: &dyn Storage, names: Names<'_>) {
    for path in names {
        match storage.remove(path) {
            Ok(()) | Err(_) => {}
        }
    }
}

fn write_sections(storage: &dyn Storage, path: &str, list: &List) -> Result<(), Error> {
    let mut text = String::new();
    for (key, ids) in SECTIONS.iter().zip(&list.picks.sections) {
        write_to(&mut text, format_args!("{key}\t"))?;
        for (n, id) in ids.iter().enumerate() {
            write_to(
                &mut text,
                format_args!("{}{id}", if n == 0 { "" } else { "," }),
            )?;
        }
        write_to(&mut text, format_args!("\n"))?;
    }
    storage.write(path, text.as_bytes()).map_err(|_| Error::Io)
}

fn write_head(
    storage: &dyn Storage,
    path: &str,
    runs: &[(usize, u32)],
    list: &List,
) -> Result<(), Error> {
    let mut text = String::new();
    let fair = if list.picks.fair { "fair" } else { "name" };
    write_to(&mut text, format_args!("{HEADER}\t"))?;
    for (n, (group, count)) in runs.iter().enumerate() {
        write_to(
            &mut text,
            format_args!("{}{group}:{count}", if n == 0 { "" } else { "," }),
        )?;
    }
    write_to(&mut text, format_args!("\t{fair}\t"))?;
    for (n, ids) in list.picks.sections.iter().enumerate() {
        write_to(
            &mut text,
            format_args!("{}{}", if n == 0 { "" } else { "," }, ids.len()),
        )?;
    }
    write_to(&mut text, format_args!("\n"))?;
    storage.write(path, text.as_bytes()).map_err(|_| Error::Io)
}

/// What a view keeps of each plugin: its rank, or `None` to leave it out.
pub type Keep<'a> = dyn FnMut(usize, &Entry) -> Result<Option<u32>, Error> + 'a;

/// The order a view of the plugins shows them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// The library's fair order of the day: the list's where it is fair.
    Fair,
    /// A to Z: the list's where it is not fair.
    Name,
    /// Newest first.
    Added,
    /// Most recently updated first.
    Updated,
    /// Highest rank first (as `pick` was given), then in the list's order.
    Rank,
}

impl Order {
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Order::Fair => "fair",
            Order::Name => "name",
            Order::Added => "added",
            Order::Updated => "updated",
            Order::Rank => "rank",
        }
    }
}

/// The plugins of the index on the card: in memory only the runs of plugins of
/// one group, in order, and how many each section has.
#[derive(Debug, Default)]
pub struct Index {
    runs: Vec<(usize, usize)>,
    /// The list is in the library's fair order (else by name).
    fair: bool,
    sections: [usize; SECTIONS.len()],
}

impl Index {
    /// The index kept on the card, if there is one.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn kept(storage: &dyn Storage) -> Result<Option<Index>, Error> {
        Self::open(storage, HEAD)
    }

    fn open(storage: &dyn Storage, head: &str) -> Result<Option<Index>, Error> {
        let Some(text) = line_at(storage, head, 0)? else {
            return Ok(None);
        };
        let mut fields = text.split('\t');
        if fields.next() != Some(HEADER) {
            return Ok(None);
        }
        let list = fields.next().unwrap_or("");
        let fair = fields.next() == Some("fair");
        let mut sections = [0; SECTIONS.len()];
        for (count, text) in sections
            .iter_mut()
            .zip(fields.next().unwrap_or("").split(','))
        {
            *count = text.parse().unwrap_or(0);
        }
        let mut runs = Vec::new();
        for run in list.split(',').filter(|run| !run.is_empty()) {
            let Some((group, count)) = run.split_once(':') else {
                continue;
            };
            if let (Ok(group), Ok(count)) = (group.parse::<usize>(), count.parse::<usize>())
                && group < GROUPS.len()
            {
                push(&mut runs, (group, count))?;
            }
        }
        Ok(Some(Index {
            runs,
            fair,
            sections,
        }))
    }

    /// The list is in the library's fair order: its order is [`Order::Fair`],
    /// and A to Z is sorted here.
    #[must_use]
    pub fn fair(&self) -> bool {
        self.fair
    }

    /// How many plugins section `section` (of [`SECTIONS`]) has.
    #[must_use]
    pub fn section_len(&self, section: usize) -> usize {
        self.sections.get(section).copied().unwrap_or(0)
    }

    /// The ids of the plugins of section `section`, in its order.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn section(&self, storage: &dyn Storage, section: usize) -> Result<Vec<String>, Error> {
        let mut ids = Vec::new();
        let Some(key) = SECTIONS.get(section) else {
            return Ok(ids);
        };
        let mut at = 0u32;
        while let Some(line) = line_at(storage, SECTIONS_FILE, at)? {
            at = at.saturating_add(u32::try_from(line.len()).unwrap_or(u32::MAX)) + 1;
            if let Some(list) = line.strip_prefix(key).and_then(|l| l.strip_prefix('\t')) {
                for id in list.split(',').filter(|id| !id.is_empty()) {
                    push(&mut ids, copy(id)?)?;
                }
                break;
            }
            if line.is_empty() {
                break;
            }
        }
        Ok(ids)
    }

    /// The orders a list of the index may show: the list's own first.
    #[must_use]
    pub fn orders(&self) -> &'static [Order] {
        if self.fair {
            &[Order::Fair, Order::Name, Order::Added, Order::Updated]
        } else {
            &[Order::Name, Order::Added, Order::Updated]
        }
    }

    /// The order a list shows `key` in: one of [`Index::orders`], the first
    /// where `key` is none of them.
    #[must_use]
    pub fn order(&self, key: &str) -> Order {
        let orders = self.orders();
        orders
            .iter()
            .copied()
            .find(|o| o.key() == key)
            .or_else(|| orders.first().copied())
            .unwrap_or(Order::Name)
    }

    /// The runs of plugins of one group, in order: (group, how many).
    #[must_use]
    pub fn runs(&self) -> &[(usize, usize)] {
        &self.runs
    }

    /// How many plugins there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.runs.iter().map(|(_, count)| count).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Plugin `n`, read from the card.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn entry(&self, storage: &dyn Storage, n: usize) -> Result<Option<Entry>, Error> {
        let Some(offset) = number_at(storage, OFFSETS, n) else {
            return Ok(None);
        };
        match line_at(storage, INDEX, offset)? {
            Some(line) => Entry::parse(&line),
            None => Ok(None),
        }
    }

    /// Searches the names and summaries for `query` (the case of letters left
    /// aside), keeping the hits on the card; how many there are.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn search(&self, storage: &dyn Storage, query: &str) -> Result<usize, Error> {
        let query = lowercase(query.trim())?;
        self.pick(
            storage,
            |_, entry| {
                let found = lowercase(&entry.name)?.contains(query.as_str())
                    || lowercase(&entry.summary)?.contains(query.as_str());
                Ok(found.then_some(0))
            },
            Order::Fair,
            usize::MAX,
        )
    }

    /// Each plugin with its place, read through the index once, in order. A
    /// line that is no plugin's is passed over, and still counts its place.
    ///
    /// # Errors
    /// `NoMemory`, or what `found` says.
    #[inline(never)]
    pub fn scan(
        &self,
        storage: &dyn Storage,
        found: &mut dyn FnMut(usize, Entry) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut buf = [0u8; 512];
        let mut line: Vec<u8> = Vec::new();
        let (mut at, mut n) = (0u64, 0usize);
        while let Ok(read) = storage.read_at(INDEX, at, &mut buf) {
            if read == 0 {
                break;
            }
            at += read as u64;
            for &byte in buf.get(..read).unwrap_or_default() {
                if byte != b'\n' {
                    if line.len() <= MAX_LINE {
                        put(&mut line, &[byte])?;
                    }
                    continue;
                }
                if let Some(entry) = core::str::from_utf8(&line)
                    .ok()
                    .map(Entry::parse)
                    .transpose()?
                    .flatten()
                {
                    found(n, entry)?;
                }
                line.clear();
                n += 1;
            }
        }
        Ok(())
    }

    /// Keeps the plugins `keep` takes (with a rank, for [`Order::Rank`]), in
    /// `order`, `most` of them at most, on the card (read back with [`hit`]);
    /// how many there are (none where the card did not keep them).
    ///
    /// However many plugins there are, little is held at once: in the list's
    /// order they are written as they are read; by a key, the best `most` are
    /// kept while reading where that is few, and otherwise runs of [`RUN`] are
    /// sorted, kept on the card and merged.
    ///
    /// # Errors
    /// `NoMemory`, or what `keep` says.
    pub fn pick(
        &self,
        storage: &dyn Storage,
        mut keep: impl FnMut(usize, &Entry) -> Result<Option<u32>, Error>,
        order: Order,
        most: usize,
    ) -> Result<usize, Error> {
        let mut out = Output::new(storage, HITS);
        if order == Order::Fair || (order == Order::Name && !self.fair) {
            self.scan(storage, &mut |n, entry| {
                if out.count < most && keep(n, &entry)?.is_some() {
                    out.push(u32::try_from(n).unwrap_or(u32::MAX))?;
                }
                Ok(())
            })?;
            return Ok(out.finish());
        }
        let key = |entry: &Entry, rank: u32| match order {
            Order::Added => u64::from(entry.added),
            Order::Updated => u64::from(entry.updated),
            // The first of A to Z the highest.
            Order::Name => u64::MAX - name_key(&entry.name),
            Order::Fair | Order::Rank => u64::from(rank),
        };
        if most <= RUN {
            return self.pick_best(storage, &mut keep, &key, most, out);
        }
        self.pick_merged(storage, &mut keep, &key, most, out)
    }

    /// The best few, kept in order while reading.
    fn pick_best(
        &self,
        storage: &dyn Storage,
        keep: &mut Keep<'_>,
        key: &dyn Fn(&Entry, u32) -> u64,
        most: usize,
        mut out: Output<'_>,
    ) -> Result<usize, Error> {
        let mut best: Vec<Keyed> = Vec::new();
        best.try_reserve_exact(most.saturating_add(1))
            .map_err(|_| Error::NoMemory)?;
        self.scan(storage, &mut |n, entry| {
            if let Some(rank) = keep(n, &entry)? {
                let item = (key(&entry, rank), u32::try_from(n).unwrap_or(u32::MAX));
                let at = best.partition_point(|b| before(*b, item));
                if at < most {
                    if best.len() == most {
                        best.pop();
                    }
                    best.insert(at, item);
                }
            }
            Ok(())
        })?;
        for (_, n) in best {
            out.push(n)?;
        }
        Ok(out.finish())
    }

    /// Runs sorted on the card, then merged.
    fn pick_merged(
        &self,
        storage: &dyn Storage,
        keep: &mut Keep<'_>,
        key: &dyn Fn(&Entry, u32) -> u64,
        most: usize,
        mut out: Output<'_>,
    ) -> Result<usize, Error> {
        let mut runs: Vec<u32> = Vec::new();
        let mut run: Vec<Keyed> = Vec::new();
        run.try_reserve_exact(RUN).map_err(|_| Error::NoMemory)?;
        let mut kept = storage.write(SORT, b"").is_ok();
        let scanned = self.scan(storage, &mut |n, entry| {
            if let Some(rank) = keep(n, &entry)? {
                run.push((key(&entry, rank), u32::try_from(n).unwrap_or(u32::MAX)));
                if run.len() == RUN {
                    kept &= flush(storage, &mut run, &mut runs)?;
                }
            }
            Ok(())
        });
        if let Err(error) = scanned {
            let _ = storage.remove(SORT);
            return Err(error);
        }
        if !run.is_empty() {
            match flush(storage, &mut run, &mut runs) {
                Ok(done) => kept &= done,
                Err(error) => {
                    // The runs file goes whatever ends the last flush.
                    let _ = storage.remove(SORT);
                    return Err(error);
                }
            }
        }
        drop(run);
        if !kept {
            let _ = storage.remove(SORT);
            return Ok(0);
        }
        let mut cursors: Vec<Cursor> = Vec::new();
        cursors
            .try_reserve_exact(runs.len())
            .map_err(|_| Error::NoMemory)?;
        let mut at = 0u64;
        for len in runs {
            cursors.push(Cursor::new(at, len));
            at += u64::from(len) * KEYED as u64;
        }
        while out.count < most {
            let mut first: Option<(usize, Keyed)> = None;
            for (i, cursor) in cursors.iter_mut().enumerate() {
                if let Some(item) = cursor.head(storage, SORT)
                    && first.is_none_or(|(_, best)| before(item, best))
                {
                    first = Some((i, item));
                }
            }
            let Some((i, (_, n))) = first else {
                break;
            };
            if let Some(cursor) = cursors.get_mut(i) {
                cursor.next();
            }
            out.push(n)?;
        }
        let _ = storage.remove(SORT);
        Ok(out.finish())
    }
}

/// Sorts `run` and appends it to the runs file; whether the card kept it.
fn flush(storage: &dyn Storage, run: &mut Vec<Keyed>, runs: &mut Vec<u32>) -> Result<bool, Error> {
    paperpaw_sdk::sort::sort_by(run, |a, b| order_of(*a, *b));
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(run.len() * KEYED)
        .map_err(|_| Error::NoMemory)?;
    for (key, n) in run.iter() {
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    let kept = storage.append(SORT, &bytes).is_ok();
    push(runs, u32::try_from(run.len()).unwrap_or(u32::MAX))?;
    run.clear();
    Ok(kept)
}

/// Letters and digits a name is sorted by: so many fit a [`name_key`].
const KEY_CHARS: u32 = 12;

/// Where `name` goes from A to Z, as a number: its first twelve letters and
/// digits, accents and case left aside, digits first and letters of other
/// scripts after z (names alike in those keep the list's order). Twelve of 38
/// values fit 64 bits.
#[must_use]
pub fn name_key(name: &str) -> u64 {
    let mut key = 0u64;
    let mut taken = 0;
    for value in name
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(sort_value)
    {
        if taken == KEY_CHARS {
            break;
        }
        key = key * 38 + value;
        taken += 1;
    }
    key * 38u64.pow(KEY_CHARS - taken)
}

/// Where `c` (in lower case) sorts: 1 to 10 for digits, 11 to 36 for the
/// letters (accented ones as their base letter), 37 for letters of other
/// scripts; `None` for what is passed over (spaces, signs).
fn sort_value(c: char) -> Option<u64> {
    let c = match c {
        'à'..='å' | 'æ' | 'ā' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è'..='ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì'..='ï' => 'i',
        'ł' => 'l',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'œ' => 'o',
        'ř' => 'r',
        'ß' | 'ś' | 'š' => 's',
        'ù'..='ü' | 'ů' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    };
    match c {
        '0'..='9' => Some(1 + u64::from(c) - u64::from('0')),
        'a'..='z' => Some(11 + u64::from(c) - u64::from('a')),
        c if c.is_alphanumeric() => Some(37),
        _ => None,
    }
}

/// The plugin of hit `n` of the last search.
#[must_use]
pub fn hit(storage: &dyn Storage, n: usize) -> Option<usize> {
    number_at(storage, HITS, n).and_then(|n| usize::try_from(n).ok())
}

/// Takes the list fetched to `fetched`: its index, kept for the next visits.
/// `None` (and the file gone) if it is no list this plugin reads: the index kept
/// before is not touched then. The index is built beside the one kept
/// (`*.part`) and moved into place, the head last: an index is whole where its
/// head is.
///
/// # Errors
/// `NoMemory`, or `Io` where the card refuses a write while the new index is
/// built (the one kept before is not touched), or a move into place (the head
/// is taken away first, so a move cut short leaves no head and so no index:
/// the next visit fetches the list again).
pub fn take(
    storage: &dyn Storage,
    fetched: &str,
    device: Device<'_>,
    hooks: Hooks<'_>,
) -> Result<Option<(Index, List)>, Error> {
    let fresh = [
        part(INDEX)?,
        part(OFFSETS)?,
        part(HEAD)?,
        part(SECTIONS_FILE)?,
    ];
    let [index, offsets, head, sections] = &fresh;
    let built = build(
        storage,
        fetched,
        [
            index.as_str(),
            offsets.as_str(),
            head.as_str(),
            sections.as_str(),
        ],
        device,
        hooks,
    );
    let _ = storage.remove(fetched);
    let Some(list) = built? else {
        return Ok(None);
    };
    // The head first away, then the rest into place, the head last.
    let _ = storage.remove(HEAD);
    for (from, to) in [
        (index, INDEX),
        (offsets, OFFSETS),
        (sections, SECTIONS_FILE),
        (head, HEAD),
    ] {
        if storage.rename(from, to).is_err() {
            return Err(Error::Io);
        }
    }
    // The hits of a search before were of the list before.
    let _ = storage.remove(HITS);
    Ok(Index::kept(storage)?.map(|index| (index, list)))
}

fn part(name: &str) -> Result<String, Error> {
    concat(&[name, ".part"])
}
