//! [`Str`]: text kept in the program or owned, in two words.

use alloc::borrow::Cow;
use alloc::string::String;
use core::fmt;

use crate::id::Text;

/// Text kept in the program or owned, in two words where a
/// `Cow<'static, str>` takes three: for what the kernel keeps for every
/// plugin and setting (names, titles, descriptions), where a word each
/// adds up. Reads as a `str`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Str(Text);

impl Str {
    /// Text kept in the program; usable in `const` and `static` items.
    pub const fn new_static(text: &'static str) -> Str {
        Str(Text::borrowed(text))
    }

    /// Text as it came: kept in the program, or owned.
    pub fn new(text: impl Into<Cow<'static, str>>) -> Str {
        Str(Text::new(text.into()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    /// The text where it is kept in the program, `None` where it is owned
    /// (see [`Id::kept`](crate::Id::kept)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn kept(&self) -> Option<&'static str> {
        self.0.kept()
    }
}

impl Default for Str {
    fn default() -> Self {
        Str::new_static("")
    }
}

impl core::ops::Deref for Str {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Str {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl core::borrow::Borrow<str> for Str {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&'static str> for Str {
    fn from(text: &'static str) -> Self {
        Str::new_static(text)
    }
}

impl From<String> for Str {
    fn from(text: String) -> Self {
        Str::new(text)
    }
}

impl From<Cow<'static, str>> for Str {
    fn from(text: Cow<'static, str>) -> Self {
        Str::new(text)
    }
}

impl From<Str> for Cow<'static, str> {
    fn from(text: Str) -> Self {
        match text.0.kept() {
            Some(kept) => Cow::Borrowed(kept),
            None => Cow::Owned(String::from(text.as_str())),
        }
    }
}

impl From<Str> for String {
    fn from(text: Str) -> Self {
        String::from(text.as_str())
    }
}

impl PartialEq<str> for Str {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Str {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<Str> for &str {
    fn eq(&self, other: &Str) -> bool {
        *self == other.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_as_text_kept_or_owned_in_two_words() {
        const KEPT: Str = Str::new_static("Clock");
        let owned = Str::new(String::from("Clock"));
        assert_eq!(KEPT, owned);
        assert_eq!(owned, "Clock");
        assert_eq!(&*owned, "Clock");
        assert_eq!(alloc::format!("{owned} {KEPT:?}"), "Clock \"Clock\"");
        assert_eq!(Str::default(), "");
        assert_eq!(Str::from(Cow::Borrowed("a")), Str::from(String::from("a")));
        assert_eq!(owned.clone().len(), 5);
        assert!(matches!(Cow::from(KEPT), Cow::Borrowed("Clock")));
        assert!(matches!(Cow::from(owned.clone()), Cow::Owned(ref o) if o == "Clock"));
        assert_eq!(
            core::mem::size_of::<Str>(),
            2 * core::mem::size_of::<usize>()
        );
        assert_eq!(
            core::mem::size_of::<Option<Str>>(),
            core::mem::size_of::<Str>()
        );
    }
}
