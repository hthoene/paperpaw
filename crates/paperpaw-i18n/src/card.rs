//! Catalogs on the card, read as needed (ADR 0033).
//!
//! A compiled catalog (`*.ppcat`, [`Catalog::compile`](crate::Catalog::compile))
//! is, all numbers little-endian:
//!
//! - a header of 20 bytes: `PPCT`, the version (1), three zero bytes, the
//!   length of the meta text, the number of messages and of blocks (each
//!   a `u32`);
//! - the meta text: the catalog's `[meta]` section as written;
//! - the block table: each block's first hash and place in the file
//!   (`u32`s), then where the last block ends;
//! - the blocks: messages sorted by the FNV-1a hash of their key, then by
//!   key, each its hash (`u32`), key length (`u8`), message length
//!   (`u16`), key and message, unescaped. At most [`BLOCK_MESSAGES`]
//!   messages and [`BLOCK_BYTES`] bytes a block (a longer message has one
//!   of its own); messages of one hash are never split between blocks.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, OnceCell};
use core::fmt;

use crate::catalog::CatalogError;

/// Where a compiled catalog is read from, e.g. a file on the card.
pub trait CatalogFile {
    /// Fills `buf` with the bytes from `offset` on; `false` where it
    /// cannot (the card was pulled, a read failed, the file is shorter).
    fn read_exact_at(&self, offset: u32, buf: &mut [u8]) -> bool;
    /// The file's size in bytes.
    fn size(&self) -> u32;
}

const MAGIC: &[u8; 4] = b"PPCT";
const VERSION: u8 = 1;
const HEADER: usize = 20;
/// The most messages a block holds.
pub const BLOCK_MESSAGES: usize = 32;
/// The most bytes a block of more than one message takes.
pub const BLOCK_BYTES: usize = 2048;
/// The longest message, in bytes.
pub const MAX_MESSAGE: usize = 4096;
/// The most bytes any block takes (one read): one of the longest messages
/// with the longest key, or messages of one hash.
const MAX_BLOCK: u32 = 8 * 1024;
/// The longest meta text.
const MAX_META: u32 = 4 * 1024;
/// The most blocks (8 bytes each in memory): some 64,000 messages.
const MAX_BLOCKS: u32 = 2048;
/// A message's place before its key: hash, key length, message length.
const ENTRY: usize = 7;
/// What the messages read may take before no more are read until the
/// kernel forgets them ([`Translator::forget_read`](crate::Translator::forget_read)).
pub const MAX_READ: usize = 16 * 1024;

/// The FNV-1a hash of `key`, by which messages are sorted.
pub(crate) fn hash(key: &str) -> u32 {
    key.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Compiles `messages` (key, message, sorted by key, each once) and the
/// meta text `meta` into a compiled catalog.
pub(crate) fn compile(meta: &str, messages: &[(&str, &str)]) -> Result<Vec<u8>, CatalogError> {
    let mut sorted: Vec<(u32, &str, &str)> =
        messages.iter().map(|&(k, m)| (hash(k), k, m)).collect();
    paperpaw_sort::sort_by(&mut sorted, |a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    // Each block's first hash, messages and bytes.
    let mut blocks: Vec<(u32, usize, Vec<u8>)> = Vec::new();
    let mut previous = None;
    for &(hash, key, message) in &sorted {
        if key.len() > usize::from(u8::MAX) || message.len() > MAX_MESSAGE {
            return Err(CatalogError::TooLarge);
        }
        let size = ENTRY + key.len() + message.len();
        let full = blocks.last().is_none_or(|(_, count, block)| {
            *count >= BLOCK_MESSAGES || block.len() + size > BLOCK_BYTES
        });
        // Messages of one hash stay in one block.
        if full && previous != Some(hash) {
            blocks.push((hash, 0, Vec::new()));
        }
        previous = Some(hash);
        let Some((_, count, block)) = blocks.last_mut() else {
            return Err(CatalogError::TooLarge);
        };
        *count += 1;
        block.extend_from_slice(&hash.to_le_bytes());
        block.push(key.len() as u8);
        block.extend_from_slice(&(message.len() as u16).to_le_bytes());
        block.extend_from_slice(key.as_bytes());
        block.extend_from_slice(message.as_bytes());
        if block.len() > MAX_BLOCK as usize {
            return Err(CatalogError::TooLarge);
        }
    }
    let too_large = |n: usize| u32::try_from(n).map_err(|_| CatalogError::TooLarge);
    if meta.len() > MAX_META as usize || blocks.len() > MAX_BLOCKS as usize {
        return Err(CatalogError::TooLarge);
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&[VERSION, 0, 0, 0]);
    out.extend_from_slice(&too_large(meta.len())?.to_le_bytes());
    out.extend_from_slice(&too_large(sorted.len())?.to_le_bytes());
    out.extend_from_slice(&too_large(blocks.len())?.to_le_bytes());
    out.extend_from_slice(meta.as_bytes());
    let mut at = too_large(out.len() + blocks.len() * 8 + 4)?;
    for (first, _, block) in &blocks {
        out.extend_from_slice(&first.to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        at = at
            .checked_add(too_large(block.len())?)
            .ok_or(CatalogError::TooLarge)?;
    }
    out.extend_from_slice(&at.to_le_bytes());
    for (_, _, block) in &blocks {
        out.extend_from_slice(block);
    }
    Ok(out)
}

/// A message in a block.
struct Entry<'a> {
    /// The bytes it takes in the block.
    size: usize,
    hash: u32,
    key: &'a [u8],
    message: &'a [u8],
}

/// The message at `at` of `block`.
fn entry_at(block: &[u8], at: usize) -> Option<Entry<'_>> {
    let head = block.get(at..at.checked_add(ENTRY)?)?;
    let hash = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let key_len = usize::from(head[4]);
    let message_len = usize::from(u16::from_le_bytes([head[5], head[6]]));
    let key_at = at + ENTRY;
    let key = block.get(key_at..key_at + key_len)?;
    let message = block.get(key_at + key_len..key_at + key_len + message_len)?;
    Some(Entry {
        size: ENTRY + key_len + message_len,
        hash,
        key,
        message,
    })
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// A compiled catalog opened: its file and its block table.
#[derive(Clone)]
pub(crate) struct Card {
    file: Rc<dyn CatalogFile>,
    /// Each block's first hash and place, then (`u32::MAX`, where the last
    /// ends); empty while unloaded.
    blocks: Box<[(u32, u32)]>,
    messages: u32,
    read: Read,
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Card")
            .field("blocks", &self.blocks.len())
            .field("messages", &self.messages)
            .field("read", &self.read.bytes.get())
            .finish_non_exhaustive()
    }
}

/// The header of a compiled catalog: its meta text, the number of
/// messages and blocks.
pub(crate) fn open(file: &dyn CatalogFile) -> Result<(String, u32), CatalogError> {
    let mut header = [0u8; HEADER];
    if !file.read_exact_at(0, &mut header) {
        return Err(CatalogError::Corrupt);
    }
    if &header[..4] != MAGIC || header[4] != VERSION {
        return Err(CatalogError::Corrupt);
    }
    let meta_len = word(&header, 8).ok_or(CatalogError::Corrupt)?;
    let messages = word(&header, 12).ok_or(CatalogError::Corrupt)?;
    if meta_len > MAX_META {
        return Err(CatalogError::Corrupt);
    }
    let mut meta = vec![0u8; meta_len as usize];
    if !file.read_exact_at(HEADER as u32, &mut meta) {
        return Err(CatalogError::Corrupt);
    }
    let meta = String::from_utf8(meta).map_err(|_| CatalogError::Corrupt)?;
    Ok((meta, messages))
}

impl Card {
    /// The catalog of `file` whose header [`open`] read; its block table
    /// read and checked.
    pub(crate) fn new(file: Rc<dyn CatalogFile>, messages: u32) -> Result<Card, CatalogError> {
        let mut card = Card {
            file,
            blocks: Box::default(),
            messages,
            read: Read::default(),
        };
        card.load()?;
        Ok(card)
    }

    /// Reads and checks the block table.
    pub(crate) fn load(&mut self) -> Result<(), CatalogError> {
        if !self.blocks.is_empty() {
            return Ok(());
        }
        let corrupt = CatalogError::Corrupt;
        let mut header = [0u8; HEADER];
        if !self.file.read_exact_at(0, &mut header) {
            return Err(corrupt);
        }
        let meta_len = word(&header, 8).ok_or(CatalogError::Corrupt)?;
        let count = word(&header, 16).ok_or(CatalogError::Corrupt)?;
        if count > MAX_BLOCKS || meta_len > MAX_META {
            return Err(corrupt);
        }
        let table_at = HEADER as u32 + meta_len;
        let mut table = vec![0u8; count as usize * 8 + 4];
        if !self.file.read_exact_at(table_at, &mut table) {
            return Err(corrupt);
        }
        let first_block = table_at + table.len() as u32;
        let end = word(&table, count as usize * 8).ok_or(CatalogError::Corrupt)?;
        if end > self.file.size() || end < first_block {
            return Err(corrupt);
        }
        let mut blocks = Vec::with_capacity(count as usize + 1);
        let mut last: Option<(u32, u32)> = None;
        for i in 0..count as usize {
            let first = word(&table, i * 8).ok_or(CatalogError::Corrupt)?;
            let at = word(&table, i * 8 + 4).ok_or(CatalogError::Corrupt)?;
            let in_order = match last {
                // Hashes go up from block to block, places too.
                Some((hash, place)) => first > hash && at > place && at - place <= MAX_BLOCK,
                None => at == first_block,
            };
            if !in_order {
                return Err(corrupt);
            }
            last = Some((first, at));
            blocks.push((first, at));
        }
        match last {
            Some((_, place)) if end <= place || end - place > MAX_BLOCK => return Err(corrupt),
            None if end != first_block => return Err(corrupt),
            _ => {}
        }
        blocks.push((u32::MAX, end));
        self.blocks = blocks.into_boxed_slice();
        Ok(())
    }

    /// Forgets the block table and the messages read.
    pub(crate) fn unload(&mut self) {
        self.blocks = Box::default();
        self.forget();
    }

    pub(crate) fn is_loaded(&self) -> bool {
        !self.blocks.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.messages as usize
    }

    /// The bytes its block table takes.
    pub(crate) fn index_on_heap(&self) -> usize {
        self.blocks.len() * 8
    }

    /// The bytes the messages read take.
    pub(crate) fn read_bytes(&self) -> usize {
        self.read.bytes.get()
    }

    /// Forgets the messages read.
    pub(crate) fn forget(&mut self) {
        self.read = Read::default();
    }

    /// The message for `key`, read from the file where it was not yet.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        let hash = hash(key);
        if let Some(read) = self.read.find(hash, key) {
            return read.message.as_deref();
        }
        if self.read.bytes.get() >= MAX_READ {
            return None;
        }
        let message = self.read_message(hash, key);
        self.read.add(hash, key, message)?.message.as_deref()
    }

    /// The message for `key` (of hash `hash`) from the file.
    fn read_message(&self, hash: u32, key: &str) -> Option<Box<str>> {
        // The last block whose first hash is not above it; the table's
        // last place is where the blocks end.
        let blocks = self.blocks.get(..self.blocks.len().checked_sub(1)?)?;
        let i = blocks
            .partition_point(|(first, _)| *first <= hash)
            .checked_sub(1)?;
        let (_, start) = blocks[i];
        let (_, end) = *self.blocks.get(i + 1)?;
        let mut block = vec![0u8; end.checked_sub(start)? as usize];
        if !self.file.read_exact_at(start, &mut block) {
            return None;
        }
        let mut at = 0;
        while let Some(entry) = entry_at(&block, at) {
            if entry.hash > hash {
                break;
            }
            if entry.hash == hash && entry.key == key.as_bytes() {
                return core::str::from_utf8(entry.message).ok().map(Box::from);
            }
            at += entry.size;
        }
        None
    }
}

/// The messages read from a catalog on the card, and those it lacks: a
/// list that only grows while the catalog is lent out, so a message
/// borrowed stays where it is; emptied with `&mut` ([`Card::forget`]).
#[derive(Default)]
struct Read {
    first: OnceCell<Box<Message>>,
    bytes: Cell<usize>,
}

struct Message {
    hash: u32,
    key: Box<str>,
    message: Option<Box<str>>,
    next: OnceCell<Box<Message>>,
}

impl Clone for Read {
    /// A copy reads anew.
    fn clone(&self) -> Read {
        Read::default()
    }
}

impl Read {
    fn iter(&self) -> impl Iterator<Item = &Message> {
        core::iter::successors(self.first.get().map(|m| &**m), |m| {
            m.next.get().map(|n| &**n)
        })
    }

    fn find(&self, hash: u32, key: &str) -> Option<&Message> {
        self.iter().find(|m| m.hash == hash && &*m.key == key)
    }

    /// Adds what was read for `key` at the end; returns it.
    fn add(&self, hash: u32, key: &str, message: Option<Box<str>>) -> Option<&Message> {
        let size =
            key.len() + message.as_ref().map_or(0, |m| m.len()) + core::mem::size_of::<Message>();
        self.bytes.set(self.bytes.get().saturating_add(size));
        let mut new = Box::new(Message {
            hash,
            key: Box::from(key),
            message,
            next: OnceCell::new(),
        });
        let mut cell = &self.first;
        loop {
            match cell.get() {
                Some(next) => cell = &next.next,
                None => match cell.set(new) {
                    Ok(()) => break,
                    Err(back) => new = back,
                },
            }
        }
        cell.get().map(|m| &**m)
    }
}

impl Drop for Read {
    /// One message after the other: a long list would take the stack.
    fn drop(&mut self) {
        let mut next = self.first.take();
        while let Some(mut message) = next {
            next = message.next.take();
        }
    }
}
