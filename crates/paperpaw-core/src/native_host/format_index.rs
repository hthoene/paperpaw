//! The device's declared formats and reading apps (`docs/abi/document.md`
//! section 1): which format reads a file, the rows of ops 19 to 21, and where a
//! format's `core.format` token lives.
//!
//! One small table, allocated entry by entry and fallibly, with the limits of
//! the contract: at most 8 formats per plugin and 32 device-wide (`ERR_FULL`).
//! Entries hold their strings whole (an id, a title key, the extensions); a
//! typical format is under 100 bytes, the worst case under 800. A format is
//! named by a never-reused [`FormatRef`], so a document opened on a format
//! that was removed and declared again does not reach the new one.
use super::{
    doc_service::FormatRef,
    format_call::Target,
    registrations::{self as wire, Record},
};
use crate::abi7::context::RowsEnd;
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{cell::Cell, ops::ControlFlow};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE};

/// Formats per plugin and on the device.
pub(crate) const PER_PLUGIN: usize = 8;
pub(crate) const DEVICE: usize = 32;
/// Reading apps per plugin and on the device (`document.md` section 1).
pub(crate) const READERS_PER_PLUGIN: usize = 4;
pub(crate) const READERS_DEVICE: usize = 16;
/// `format.spec.flags` bit 1: the format can tell its files by their start.
pub(crate) const SNIFFS: u32 = 1 << 1;

fn text(from: &str) -> Result<Box<str>, i32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(from.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.extend_from_slice(from.as_bytes());
    Ok(String::from_utf8(bytes)
        .map_err(|_| ERR_INVALID)?
        .into_boxed_str())
}

fn joined(plugin_id: &str, name: &str) -> Result<Box<str>, i32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(plugin_id.len() + 1 + name.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.extend_from_slice(plugin_id.as_bytes());
    bytes.push(b'.');
    bytes.extend_from_slice(name.as_bytes());
    Ok(String::from_utf8(bytes)
        .map_err(|_| ERR_INVALID)?
        .into_boxed_str())
}

struct Format {
    reference: FormatRef,
    plugin: u64,
    id: Box<str>,
    /// The bytes of `id` that are the plugin's id (the rest is `.name`).
    plugin_len: usize,
    title: Box<str>,
    extensions: Box<str>,
    flags: u32,
    priority: i32,
    token: u32,
}

struct Reader {
    id: Box<str>,
    screen: Box<str>,
    formats: Box<str>,
    title: Box<str>,
}

/// The user's format choice for an extension: calls the visitor with the
/// chosen format id, if any.
pub(crate) type UserChoice<'a> = dyn Fn(&str, &mut dyn FnMut(&str)) + 'a;

/// Declared formats and readers, in declaration (load) order.
pub(crate) struct FormatIndex {
    formats: Vec<Format>,
    readers: Vec<Reader>,
    next_ref: Cell<u32>,
    /// Changes whenever a row is added or removed; never 0.
    generation: Cell<u32>,
}

/// A page of rows: the position after it and the identity it was cut from.
type Rows<'a> = &'a mut dyn FnMut(Record<'_>) -> ControlFlow<()>;

pub(crate) fn extensions_of(list: &str) -> impl Iterator<Item = &str> {
    list.split('\n').filter(|_| !list.is_empty())
}

/// Whether `path` ends in `.extension`, ignoring ASCII case (FAT does).
pub(crate) fn has_extension(path: &str, extension: &str) -> bool {
    let (path, extension) = (path.as_bytes(), extension.as_bytes());
    path.len() > extension.len()
        && path[path.len() - extension.len() - 1] == b'.'
        && path[path.len() - extension.len()..].eq_ignore_ascii_case(extension)
}

impl FormatIndex {
    pub(crate) const fn new() -> Self {
        Self {
            formats: Vec::new(),
            readers: Vec::new(),
            next_ref: Cell::new(1),
            generation: Cell::new(1),
        }
    }

    fn changed(&self) {
        // Never 0: a wrap skips it.
        let next = self.generation.get().wrapping_add(1);
        self.generation.set(if next == 0 { 1 } else { next });
    }

    /// Admit one `format.spec` of `plugin` (its registry key) whose syntax is
    /// checked already.
    ///
    /// # Errors
    /// `ERR_FULL` over 8 formats of the plugin or 32 on the device,
    /// `ERR_INVALID` for a name already taken or an extension the plugin
    /// already claims, `ERR_NO_MEMORY`.
    pub(crate) fn declare_format(
        &mut self,
        plugin: u64,
        plugin_id: &str,
        spec: &wire::FormatSpec<'_>,
    ) -> Result<FormatRef, i32> {
        let mine = self.formats.iter().filter(|f| f.plugin == plugin);
        if self.formats.len() >= DEVICE || mine.clone().count() >= PER_PLUGIN {
            return Err(ERR_FULL);
        }
        let id = joined(plugin_id, spec.name)?;
        if self.formats.iter().any(|f| f.id == id) {
            return Err(ERR_INVALID);
        }
        // Unique within the plugin: one claim, one row.
        for extension in extensions_of(spec.extensions) {
            if mine
                .clone()
                .any(|f| extensions_of(&f.extensions).any(|claimed| claimed == extension))
            {
                return Err(ERR_INVALID);
            }
        }
        let reference = FormatRef(self.next_ref.get());
        let next = reference.0.checked_add(1).ok_or(ERR_NO_SPACE)?;
        let format = Format {
            reference,
            plugin,
            id,
            plugin_len: plugin_id.len(),
            title: text(spec.title)?,
            extensions: text(spec.extensions)?,
            flags: spec.flags,
            priority: spec.priority,
            token: spec.token,
        };
        self.formats
            .try_reserve_exact(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        self.formats.push(format);
        self.next_ref.set(next);
        self.changed();
        Ok(reference)
    }

    /// Admit one `reader.spec`. `title` is the app's name for its row.
    ///
    /// # Errors
    /// `ERR_INVALID` for a name already taken, `ERR_NO_MEMORY`.
    pub(crate) fn declare_reader(
        &mut self,
        plugin_id: &str,
        spec: &wire::ReaderSpec<'_>,
        title: &str,
    ) -> Result<(), i32> {
        let id = joined(plugin_id, spec.name)?;
        if self.readers.iter().any(|r| r.id == id) {
            return Err(ERR_INVALID);
        }
        let reader = Reader {
            id,
            screen: text(spec.screen)?,
            formats: text(spec.formats)?,
            title: text(title)?,
        };
        self.readers
            .try_reserve_exact(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        self.readers.push(reader);
        self.changed();
        Ok(())
    }

    /// The plugin stopped or was removed: everything it declared goes. Returns
    /// the references of its formats, so that their documents can be ended
    /// (`format_stopped`).
    pub(crate) fn remove_plugin(
        &mut self,
        plugin: u64,
        plugin_id: &str,
        mut gone: impl FnMut(FormatRef),
    ) {
        let before = (self.formats.len(), self.readers.len());
        self.formats.retain(|f| {
            let mine = f.plugin == plugin;
            if mine {
                gone(f.reference);
            }
            !mine
        });
        self.readers.retain(|r| {
            r.id.strip_prefix(plugin_id)
                .is_none_or(|rest| !rest.starts_with('.'))
        });
        if before != (self.formats.len(), self.readers.len()) {
            self.changed();
        }
    }

    fn find(&self, reference: FormatRef) -> Option<&Format> {
        self.formats.iter().find(|f| f.reference == reference)
    }

    /// The format with this id.
    pub(crate) fn by_id(&self, id: &str) -> Option<FormatRef> {
        self.formats
            .iter()
            .find(|f| &*f.id == id)
            .map(|f| f.reference)
    }

    /// Where the format's `core.format` token lives, or `None` once it is gone.
    pub(crate) fn target(&self, reference: FormatRef) -> Option<Target> {
        self.find(reference).map(|f| Target {
            plugin: f.plugin,
            token: f.token,
        })
    }

    /// Whether the format reads its book with `flags` bit `bit` set.
    pub(crate) fn has_flag(&self, reference: FormatRef, bit: u32) -> bool {
        self.find(reference).is_some_and(|f| f.flags & bit != 0)
    }

    /// Each format reference of the plugin `owner`; nothing changes.
    pub(crate) fn formats_of(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        for format in self.formats.iter().filter(|f| f.plugin == owner) {
            visit(format.reference);
        }
    }

    /// The formats of the plugin `owner` go (it is retiring); `gone` hears
    /// each reference. Its readers are the next sync's.
    pub(crate) fn remove_formats_of(&mut self, owner: u64, mut gone: impl FnMut(FormatRef)) {
        let before = self.formats.len();
        self.formats.retain(|f| {
            let mine = f.plugin == owner;
            if mine {
                gone(f.reference);
            }
            !mine
        });
        if self.formats.len() != before {
            self.changed();
        }
    }

    /// The format's id, for the host's own use.
    /// The id of the plugin that declared the format.
    pub(crate) fn plugin_id(
        &self,
        reference: FormatRef,
        visit: &mut dyn FnMut(&str),
    ) -> Result<(), i32> {
        let format = self.find(reference).ok_or(ERR_STATE)?;
        visit(format.id.get(..format.plugin_len).ok_or(ERR_STATE)?);
        Ok(())
    }

    pub(crate) fn id(&self, reference: FormatRef, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        visit(&self.find(reference).ok_or(ERR_STATE)?.id);
        Ok(())
    }

    /// Which format reads `path` (`document.md` section 1): the one named by
    /// `requested` when given; else by the longest extension; of the formats
    /// that claim it, the user's `choice` (a format id), then the higher
    /// priority, then the first loaded. `None` where no extension matches (the
    /// `sniffs` formats are tried then: see [`FormatIndex::sniffers`]).
    pub(crate) fn select(
        &self,
        path: &str,
        requested: &str,
        choice: &UserChoice<'_>,
    ) -> Result<Option<FormatRef>, i32> {
        if !requested.is_empty() {
            return self.by_id(requested).map(Some).ok_or(ERR_STATE);
        }
        let mut longest = "";
        for format in &self.formats {
            for extension in extensions_of(&format.extensions) {
                if extension.len() > longest.len() && has_extension(path, extension) {
                    longest = extension;
                }
            }
        }
        if longest.is_empty() {
            return Ok(None);
        }
        let claims = |f: &&Format| extensions_of(&f.extensions).any(|e| e == longest);
        let mut chosen = None;
        choice(longest, &mut |wanted| {
            chosen = self
                .formats
                .iter()
                .filter(claims)
                .find(|f| &*f.id == wanted)
                .map(|f| f.reference);
        });
        if chosen.is_some() {
            return Ok(chosen);
        }
        // The highest priority; the first loaded among equals (a stable max).
        let mut best: Option<&Format> = None;
        for format in self.formats.iter().filter(claims) {
            if best.is_none_or(|b| format.priority > b.priority) {
                best = Some(format);
            }
        }
        Ok(best.map(|f| f.reference))
    }

    /// The `sniffs` formats in priority order (higher first, the first loaded
    /// among equals), for a file whose extension no format has.
    pub(crate) fn sniffers(&self, mut visit: impl FnMut(FormatRef) -> ControlFlow<()>) {
        let mut list: [Option<&Format>; DEVICE] = [None; DEVICE];
        let sniffs = self.formats.iter().filter(|f| f.flags & SNIFFS != 0);
        let mut count = 0;
        for (slot, format) in list.iter_mut().zip(sniffs) {
            *slot = Some(format);
            count += 1;
        }
        let list = &mut list[..count];
        // The unique reference is the last key, so the order is total.
        paperpaw_sort::sort_by(list, |a, b| match (a, b) {
            (Some(a), Some(b)) => b
                .priority
                .cmp(&a.priority)
                .then(a.reference.0.cmp(&b.reference.0)),
            _ => core::cmp::Ordering::Equal,
        });
        for format in list.iter().flatten() {
            if visit(format.reference).is_break() {
                break;
            }
        }
    }

    /// Whether a page named a `cursor` under a `generation` this index no
    /// longer has (never 0): the caller starts again.
    fn check(&self, generation: u32) -> Result<(), i32> {
        if generation != 0 && generation != self.generation.get() {
            Err(ERR_STATE)
        } else {
            Ok(())
        }
    }

    /// Op 19: the formats in declaration order, from row `cursor`; `visit`
    /// stops the page by breaking.
    pub(crate) fn formats(
        &self,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.check(generation)?;
        let total = u32::try_from(self.formats.len()).map_err(|_| ERR_NO_SPACE)?;
        let mut next = cursor.min(total);
        for format in self.formats.iter().skip(next as usize) {
            let row = Record::ServiceDocFormat(wire::ServiceDocFormat {
                id: &format.id,
                title: &format.title,
                extensions: &format.extensions,
                flags: format.flags,
                priority: format.priority,
            });
            if visit(row).is_break() {
                break;
            }
            next += 1;
        }
        Ok(RowsEnd {
            total,
            generation: self.generation.get(),
            next,
        })
    }

    /// Op 21: the reading apps of one format id, or of all (`scope` empty). An
    /// app that lists no format reads every one.
    pub(crate) fn readers(
        &self,
        scope: &str,
        cursor: u32,
        generation: u32,
        visit: Rows<'_>,
    ) -> Result<RowsEnd, i32> {
        self.check(generation)?;
        let shown = |r: &&Reader| {
            scope.is_empty() || r.formats.is_empty() || r.formats.split('\n').any(|f| f == scope)
        };
        let total =
            u32::try_from(self.readers.iter().filter(shown).count()).map_err(|_| ERR_NO_SPACE)?;
        let mut next = cursor.min(total);
        for reader in self.readers.iter().filter(shown).skip(next as usize) {
            let row = Record::ServiceDocReader(wire::ServiceDocReader {
                id: &reader.id,
                screen: &reader.screen,
                formats: &reader.formats,
                title: &reader.title,
            });
            if visit(row).is_break() {
                break;
            }
            next += 1;
        }
        Ok(RowsEnd {
            total,
            generation: self.generation.get(),
            next,
        })
    }

    /// Format-specific facts for op 20's reply.
    pub(crate) fn row(
        &self,
        reference: FormatRef,
        visit: &mut dyn FnMut(Record<'_>),
    ) -> Result<(), i32> {
        let format = self.find(reference).ok_or(ERR_STATE)?;
        visit(Record::ServiceDocFormat(wire::ServiceDocFormat {
            id: &format.id,
            title: &format.title,
            extensions: &format.extensions,
            flags: format.flags,
            priority: format.priority,
        }));
        Ok(())
    }
}

#[path = "format_index/sync.rs"]
mod sync;

#[cfg(test)]
#[path = "format_index_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "format_index/sync_tests.rs"]
mod sync_tests;
