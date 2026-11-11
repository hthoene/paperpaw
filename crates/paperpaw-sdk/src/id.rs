use alloc::borrow::Borrow;
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;
use core::ptr::NonNull;

/// A namespaced identifier such as `core.home` or `diagnostics.screen`.
///
/// Ids consist of one or more segments of lowercase ASCII letters, digits
/// and `-`, separated by `.`. The first segment is the namespace: the id of
/// the plugin that owns the contribution.
///
/// A segment may end in `@` and a number for one placement of something
/// placed more than once, e.g. `widgets.clock@2` for a clock's second
/// place and `widgets.clock@2.face` for that place's own setting (see
/// [`Id::placement`], [`Id::unplaced`]).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(Text);

/// Text kept in the program or owned, in two words (a `Cow<str>` takes
/// three): where it starts, and its length, whose top bit says it is
/// owned (a `Box<str>`, freed with it). Ids are kept by the thousand,
/// and so are the names menus give them.
pub(crate) struct Text {
    start: NonNull<u8>,
    len: usize,
}

/// In [`Text::len`]: the text is owned.
const OWNED: usize = 1 << (usize::BITS - 1);

impl Text {
    pub(crate) const fn borrowed(text: &'static str) -> Self {
        // SAFETY: a reference's pointer is never null.
        let start = unsafe { NonNull::new_unchecked(text.as_ptr().cast_mut()) };
        Self {
            start,
            len: text.len(),
        }
    }

    fn owned(text: String) -> Self {
        let text: Box<str> = text.into_boxed_str();
        let len = text.len();
        // SAFETY: a box's pointer is never null.
        let start = unsafe { NonNull::new_unchecked(Box::into_raw(text).cast::<u8>()) };
        Self {
            start,
            len: len | OWNED,
        }
    }

    /// `text` as it came: in the program, or owned.
    pub(crate) fn new(text: alloc::borrow::Cow<'static, str>) -> Self {
        match text {
            alloc::borrow::Cow::Borrowed(text) => Self::borrowed(text),
            alloc::borrow::Cow::Owned(text) => Self::owned(text),
        }
    }

    const fn is_owned(&self) -> bool {
        self.len & OWNED != 0
    }

    /// The text where it is kept in the program, not owned.
    pub(crate) const fn kept(&self) -> Option<&'static str> {
        if self.is_owned() {
            return None;
        }
        // SAFETY: not owned: text kept in the program.
        unsafe {
            let bytes = core::slice::from_raw_parts(self.start.as_ptr(), self.len);
            Some(core::str::from_utf8_unchecked(bytes))
        }
    }

    const fn as_str(&self) -> &str {
        // SAFETY: made from a `str` of this length, which lives as long as
        // this (the program's, or the box this owns).
        unsafe {
            let bytes = core::slice::from_raw_parts(self.start.as_ptr(), self.len & !OWNED);
            core::str::from_utf8_unchecked(bytes)
        }
    }
}

impl AsRef<str> for Text {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl Drop for Text {
    fn drop(&mut self) {
        if self.is_owned() {
            let len = self.len & !OWNED;
            let raw = core::ptr::slice_from_raw_parts_mut(self.start.as_ptr(), len) as *mut str;
            // SAFETY: made by `Text::owned` from this box, dropped once.
            drop(unsafe { Box::from_raw(raw) });
        }
    }
}

impl Text {
    /// A copy that refuses where the heap does: kept text copies its two
    /// words, owned text takes one allocation of exactly its length.
    fn try_clone(&self) -> Option<Self> {
        if !self.is_owned() {
            return Some(Self {
                start: self.start,
                len: self.len,
            });
        }
        Self::try_copy(self.as_str())
    }

    /// Owned text copied from `text`, refusing where the heap does: one
    /// allocation of exactly its length (none for empty text).
    fn try_copy(text: &str) -> Option<Self> {
        if text.is_empty() {
            return Some(Self::owned(String::new()));
        }
        let layout = core::alloc::Layout::for_value(text.as_bytes());
        // SAFETY: a nonzero layout (`text` is not empty).
        let start = NonNull::new(unsafe { alloc::alloc::alloc(layout) })?;
        // SAFETY: a fresh allocation of `text.len()` bytes, disjoint from
        // `text`; the copy is UTF-8 as `text` is.
        unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), start.as_ptr(), text.len()) };
        // The layout a `Box<str>` of this length frees with (`Drop`).
        Some(Self {
            start,
            len: text.len() | OWNED,
        })
    }
}

impl Clone for Text {
    fn clone(&self) -> Self {
        if self.is_owned() {
            Self::owned(String::from(self.as_str()))
        } else {
            Self {
                start: self.start,
                len: self.len,
            }
        }
    }
}

// SAFETY: as a `Cow<'static, str>`: text never changed, shared or owned.
unsafe impl Send for Text {}
// SAFETY: the same.
unsafe impl Sync for Text {}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Text {}

impl PartialOrd for Text {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Text {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl core::hash::Hash for Text {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

/// Returned when parsing an invalid [`Id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ids are lowercase segments of [a-z0-9-] separated by '.'")
    }
}

impl core::error::Error for InvalidId {}

/// Why [`Id::try_parse`] made no id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// Not an id ([`InvalidId`]).
    Invalid,
    /// A valid id, but the heap could not hold its copy.
    NoMemory,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => InvalidId.fmt(f),
            Self::NoMemory => f.write_str("no memory for the id"),
        }
    }
}

impl core::error::Error for IdError {}

impl Id {
    /// Creates an id from a string literal.
    ///
    /// # Panics
    ///
    /// Panics if `id` is invalid. Use it in `const` items, where the check
    /// happens at compile time.
    pub const fn new_static(id: &'static str) -> Id {
        assert!(is_valid(id), "invalid id");
        Id(Text::borrowed(id))
    }

    /// Parses an id at runtime, e.g. from a configuration file.
    pub fn parse(id: &str) -> Result<Id, InvalidId> {
        if is_valid(id) {
            Ok(Id(Text::owned(String::from(id))))
        } else {
            Err(InvalidId)
        }
    }

    /// [`Id::parse`] that never aborts on a full heap, for hosts that must
    /// not abort when memory runs out (the X4's plugin admission): checks
    /// the id first ([`IdError::Invalid`]), then copies it in one allocation
    /// of its length, [`IdError::NoMemory`] where the heap refuses it.
    pub fn try_parse(id: &str) -> Result<Id, IdError> {
        if !is_valid(id) {
            return Err(IdError::Invalid);
        }
        Text::try_copy(id).map(Id).ok_or(IdError::NoMemory)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// A copy that never aborts on a full heap: `None` where the heap
    /// cannot hold it, for hosts that must not abort when memory runs out
    /// (the X4's plugin loader). An id made with [`Id::new_static`] copies
    /// without allocating; a parsed one takes one allocation of its length.
    /// [`Clone`] gives the same copy and aborts instead.
    #[must_use]
    pub fn try_clone(&self) -> Option<Id> {
        self.0.try_clone().map(Id)
    }

    /// The id as text kept in the program, for specs kept there too
    /// (e.g. `SettingSpec::number(MARGIN.static_str(), …)`, see
    /// a `static` list): for ids made
    /// with [`Id::new_static`]; empty for ids parsed at run time. Takes
    /// the id, so it can be used in `const` and `static` items.
    pub const fn static_str(self) -> &'static str {
        let text = self.static_text();
        core::mem::forget(self);
        text
    }

    /// The same, the id kept.
    pub(crate) const fn static_text(&self) -> &'static str {
        if self.0.is_owned() {
            return "";
        }
        // SAFETY: not owned: text kept in the program.
        unsafe {
            let bytes = core::slice::from_raw_parts(self.0.start.as_ptr(), self.0.len);
            core::str::from_utf8_unchecked(bytes)
        }
    }

    /// The first segment, i.e. the owning plugin's id.
    pub fn namespace(&self) -> &str {
        self.as_str().split('.').next().unwrap_or_default()
    }

    /// The id without its placement (`widgets.clock@2.face` is
    /// `widgets.clock.face`), if it names one; see [`Id::placement`].
    pub fn unplaced(&self) -> Option<Id> {
        unplaced(self.as_str()).map(|plain| Id(Text::owned(plain)))
    }

    /// The id of placement `n` of this: `widgets.clock` and 2 make
    /// `widgets.clock@2`, and so does `widgets.clock@5` (another
    /// placement of the same).
    pub fn placement(&self, n: u32) -> Id {
        let id = self.as_str();
        let last = id.rfind('.').map_or(0, |dot| dot + 1);
        let end = id[last..].find('@').map_or(id.len(), |at| last + at);
        Id(Text::owned(alloc::format!("{}@{n}", &id[..end])))
    }

    /// Whether this id belongs to the plugin `plugin`.
    pub fn is_owned_by(&self, plugin: &Id) -> bool {
        self.namespace() == plugin.as_str()
    }
}

impl Borrow<str> for Id {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<str> for Id {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&Id> for Id {
    fn from(id: &Id) -> Id {
        id.clone()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.as_str())
    }
}

pub(crate) const fn is_valid(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut segment_start = true;
    // After `@`: digits only, at least one, to the segment's end.
    let mut placement = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'.' {
            if segment_start {
                return false;
            }
            segment_start = true;
            placement = false;
        } else if b == b'@' {
            if segment_start || placement || i + 1 >= bytes.len() {
                return false;
            }
            placement = true;
            segment_start = true;
        } else if placement && !b.is_ascii_digit() {
            return false;
        } else if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            segment_start = false;
        } else {
            return false;
        }
        i += 1;
    }
    !segment_start
}

/// Where `id` names a placement (`@` and its number), the id without it.
pub(crate) fn unplaced(id: &str) -> Option<String> {
    let at = id.find('@')?;
    let end = id[at..].find('.').map_or(id.len(), |dot| at + dot);
    let mut plain = String::with_capacity(id.len());
    plain.push_str(&id[..at]);
    plain.push_str(&id[end..]);
    Some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_are_ids_of_their_own() {
        for valid in ["widgets.clock@2", "widgets.clock@12.face", "a@1"] {
            assert!(Id::parse(valid).is_ok(), "{valid}");
        }
        for invalid in ["widgets.clock@", "widgets.@2", "a@2b", "a@1@2", "@1"] {
            assert!(Id::parse(invalid).is_err(), "{invalid}");
        }
        let id = Id::parse("widgets.clock@2.face").unwrap();
        assert_eq!(id.unplaced(), Some(Id::new_static("widgets.clock.face")));
        assert_eq!(Id::new_static("widgets.clock").unplaced(), None);
        let clock = Id::new_static("widgets.clock");
        assert_eq!(clock.placement(3).as_str(), "widgets.clock@3");
        // Another placement of a placed id, not one of it.
        let placed = clock.placement(2).placement(3);
        assert_eq!(placed.as_str(), "widgets.clock@3");
        assert!(Id::parse(placed.as_str()).is_ok());
        assert_eq!(id.placement(4).as_str(), "widgets.clock@2.face@4");
    }

    #[test]
    fn validates_ids() {
        for valid in ["core", "core.home", "my-plugin.screen-2", "a.b.c"] {
            assert!(Id::parse(valid).is_ok(), "{valid}");
        }
        for invalid in ["", ".", "a.", ".a", "a..b", "Core.home", "a b", "a_b"] {
            assert!(Id::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn namespace_and_ownership() {
        const ID: Id = Id::new_static("diagnostics.screen");
        assert_eq!(ID.namespace(), "diagnostics");
        assert!(ID.is_owned_by(&Id::new_static("diagnostics")));
        assert!(!ID.is_owned_by(&Id::new_static("diag")));
    }

    #[test]
    fn static_and_parsed_ids_are_equal() {
        assert_eq!(Id::new_static("core.home"), Id::parse("core.home").unwrap());
    }

    /// Two words, and none more where it is optional.
    #[test]
    fn ids_take_two_words() {
        assert_eq!(size_of::<Id>(), 2 * size_of::<usize>());
        assert_eq!(size_of::<Option<Id>>(), size_of::<Id>());
        let owned = Id::parse("a.b").unwrap();
        let copy = owned.clone();
        drop(owned);
        assert_eq!(copy.as_str(), "a.b");
        assert_eq!(Id::new_static("a.b").static_str(), "a.b");
        assert_eq!(copy.static_str(), "");
    }
}
