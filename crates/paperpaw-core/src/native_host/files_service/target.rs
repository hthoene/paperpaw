//! Roots and paths of `core.files` (`files.md` 1 and 3): which root, which
//! feature, the path rules and the root-1 exclusions, in the order the contract
//! lists them, then the path joined for the storage in a stack buffer.
use super::View;
use crate::native_host::storage::{MAX_FOLDER, MAX_JOINED, PREFIX, relative_path};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NOT_FOUND, ERR_UNSUPPORTED};

/// Root 0: the plugin's data folder.
pub(super) const DATA: u32 = 0;
/// Root 1: the user's files on the card.
pub(super) const CARD: u32 = 1;
/// Root 2: the plugin's install folder, read only.
pub(super) const BUNDLE: u32 = 2;
/// Root 3: the book of the format call the caller is in, read only
/// (`document.md` "Roots").
pub(super) const BOOK: u32 = 3;

/// The host's folder in root 0, where served request bodies arrive.
const RECEIVED: &str = ".received";

/// What a call does to the path, for the root rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Intent {
    Read,
    Change,
}

/// A path of a root, checked, and joined for the storage.
pub(super) struct Target {
    buf: [u8; MAX_JOINED],
    len: usize,
    /// Where the plugin's part of the path starts in `buf`.
    start: usize,
    /// The length of the root's own folder in `buf`.
    base: usize,
    pub(super) root: u32,
    /// Whether the path is `.received` or below it (root 0).
    received: bool,
    /// Root 2 of a built-in plugin: an install folder that counts as empty.
    pub(super) empty_bundle: bool,
}

impl Target {
    /// A target not yet resolved; [`resolve`] fills it.
    pub(super) const EMPTY: Self = Self {
        buf: [0; MAX_JOINED],
        len: 0,
        start: 0,
        base: 0,
        root: 0,
        received: false,
        empty_bundle: false,
    };

    /// The path for the storage, from the card's root.
    pub(super) fn joined(&self) -> &str {
        // Built from `&str` parts at their boundaries below.
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or_default()
    }

    /// The path the plugin gave, `""` for its root.
    pub(super) fn rel(&self) -> &str {
        core::str::from_utf8(&self.buf[self.start..self.len]).unwrap_or_default()
    }

    pub(super) fn is_root(&self) -> bool {
        self.start == self.len
    }

    /// Whether the path is `.received` of root 0 or below it: the host's.
    pub(super) fn is_received(&self) -> bool {
        self.received
    }

    /// Whether the path is root 0's `.received` itself.
    pub(super) fn is_received_folder(&self) -> bool {
        self.received && !self.rel().contains('/')
    }

    /// The folder above the path in the plugin's part (`""` for the root).
    pub(super) fn parent_rel(&self) -> &str {
        self.rel().rsplit_once('/').map_or("", |(parent, _)| parent)
    }

    /// The joined path of the folder above the path, `None` for a root.
    pub(super) fn parent_joined(&self) -> Option<&str> {
        if self.is_root() {
            return None;
        }
        let joined = self.joined();
        Some(match self.rel().rsplit_once('/') {
            Some(_) => joined.rsplit_once('/').map_or("", |(parent, _)| parent),
            None => &joined[..self.base],
        })
    }

    /// Whether the path is the top of the card (root 1 `""`, or root 3 `""`
    /// of a book whose folder is the card's top): where the kernel's folders
    /// are hidden and refused.
    pub(super) fn is_card_top(&self) -> bool {
        self.is_root() && (self.root == CARD || (self.root == BOOK && self.base == 0))
    }

    /// The joined path of the root itself (`""` for the card).
    pub(super) fn base(&self) -> &str {
        &self.joined()[..self.base]
    }

    /// The last segment of the path, `""` for a root.
    pub(super) fn name(&self) -> &str {
        let rel = self.rel();
        rel.rsplit_once('/').map_or(rel, |(_, name)| name)
    }
}

/// Resolves `path` of `root` for the caller, checking in the contract's
/// order: the root (`ERR_UNSUPPORTED` from 4 on, and for 3 outside a call of
/// a `folder` or `siblings` format, `ERR_DENIED` without
/// `uses.storage` on root 1 and for a change on root 2), the path rules
/// (`ERR_INVALID`, `ERR_UTF8`, `ERR_NO_SPACE`), the root-1 exclusions
/// (`ERR_INVALID`). The path may be `""`, the root.
#[inline(never)]
pub(super) fn resolve(
    view: &View<'_>,
    target: &mut Target,
    root: u32,
    path: &str,
    intent: Intent,
) -> Result<(), i32> {
    if root == BOOK {
        return resolve_book(view, target, path, intent);
    }
    match root {
        DATA => {}
        CARD => view.who.storage?,
        BUNDLE if intent == Intent::Change => return Err(ERR_DENIED),
        BUNDLE => {}
        _ => return Err(ERR_UNSUPPORTED),
    }
    if !path.is_empty() {
        relative_path(path.as_bytes())?;
    }
    let first = path.split('/').next().unwrap_or_default();
    if root == CARD && !first.is_empty() && reaches_protected(view, first) {
        return Err(ERR_INVALID);
    }
    // Filled in the caller's place: a path is 640 bytes, and a copy of it per
    // return would put several of them in the frame of an op with two.
    target.len = 0;
    target.start = 0;
    target.base = 0;
    target.root = root;
    target.received = false;
    target.empty_bundle = false;
    let (kind, folder) = match root {
        DATA => (PREFIX, Some(view.who.plugin)),
        BUNDLE => ("plugins/", Some(view.who.plugin)),
        _ => ("", None),
    };
    if let Some(id) = folder {
        // The id is the host's, stamped from the owner; it must still be
        // one segment that leaves room for every permitted path.
        if kind.len() + id.len() + 1 > MAX_FOLDER
            || id.contains('/')
            || relative_path(id.as_bytes()).is_err()
        {
            return Err(ERR_INVALID);
        }
        push(target, kind.as_bytes())?;
        push(target, id.as_bytes())?;
        target.base = target.len;
        // `.received` of the data folder (the host's), by spelling or by the
        // storage's identity: the folder it is in is the data folder.
        target.received = root == DATA
            && !first.is_empty()
            && (same_fold(first, RECEIVED) || view.host.excluded(DATA, target.joined(), first));
        if !path.is_empty() {
            push(target, b"/")?;
        }
    }
    target.start = target.len;
    push(target, path.as_bytes())?;
    // A built-in plugin has no install folder: what the card holds there (a
    // leftover of an older install) is never read.
    if root == BUNDLE && view.host.built_in(view.who.plugin) {
        if !path.is_empty() {
            return Err(ERR_NOT_FOUND);
        }
        target.empty_bundle = true;
    }
    Ok(())
}

/// Root 3: the folder of the book the caller's format call is for. A caller
/// that is in no such call, or whose format reads a single file, has no root
/// 3 (`ERR_UNSUPPORTED`); a change is `ERR_DENIED`; the path rules are those
/// of the other roots, and nothing resolves above the folder (no `..`).
#[inline(never)]
fn resolve_book(
    view: &View<'_>,
    target: &mut Target,
    path: &str,
    intent: Intent,
) -> Result<(), i32> {
    let mut result = Err(ERR_UNSUPPORTED);
    view.scope.current(view.who.owner, &mut |book| {
        let Some(folder) = book.folder() else {
            return;
        };
        result = fill_book(view, target, folder, path, intent);
    });
    result
}

fn fill_book(
    view: &View<'_>,
    target: &mut Target,
    folder: &str,
    path: &str,
    intent: Intent,
) -> Result<(), i32> {
    if intent == Intent::Change {
        return Err(ERR_DENIED);
    }
    if !path.is_empty() {
        relative_path(path.as_bytes())?;
    }
    // A book at the top of the card must not reach the kernel's folders.
    let first = path.split('/').next().unwrap_or_default();
    if folder.is_empty() && !first.is_empty() && reaches_protected(view, first) {
        return Err(ERR_INVALID);
    }
    target.len = 0;
    target.start = 0;
    target.base = 0;
    target.root = BOOK;
    target.received = false;
    push(target, folder.as_bytes())?;
    target.base = target.len;
    if !path.is_empty() && !folder.is_empty() {
        push(target, b"/")?;
    }
    target.start = target.len;
    push(target, path.as_bytes())
}

fn push(target: &mut Target, bytes: &[u8]) -> Result<(), i32> {
    let end = target.len + bytes.len();
    target
        .buf
        .get_mut(target.len..end)
        .ok_or(ERR_INVALID)?
        .copy_from_slice(bytes);
    target.len = end;
    Ok(())
}

/// Whether a first segment of root 1 is `plugins`, `.paperpaw` or
/// `paperpaw.ini`, by spelling in any case or by identity (the host).
pub(super) fn reaches_protected(view: &View<'_>, first: &str) -> bool {
    super::super::owner_paths::protected_first(first) || view.host.excluded(CARD, "", first)
}

/// Whether `a` and `b` are one name to the FAT driver: in upper case, the
/// whole name (so `ı` is `I`, `ſ` is `S` and `ß` is `SS`), exactly as
/// `paperpaw-fat` compares. Used for the names the host keeps for itself,
/// whatever the storage does with case: a spelling the driver would take for
/// one of them must not get past.
pub(crate) fn same_fold(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_uppercase)
        .eq(b.chars().flat_map(char::to_uppercase))
}

/// Whether `path` is `folder` or below it, name by name; `ignore_case` for a
/// storage that takes names differing in case for one, folded as the driver
/// does ([`same_fold`]).
pub(crate) fn within(ignore_case: bool, path: &str, folder: &str) -> bool {
    let mut parts = path.split('/');
    folder.split('/').all(|name| {
        parts.next().is_some_and(|part| {
            if ignore_case {
                same_fold(part, name)
            } else {
                part == name
            }
        })
    })
}

/// Whether the two paths are spelt alike (`ignore_case` for FAT, folded as
/// the driver does).
pub(super) fn same_spelling(ignore_case: bool, a: &str, b: &str) -> bool {
    if ignore_case { same_fold(a, b) } else { a == b }
}
