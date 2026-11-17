//! The start of the text a mark kept, to tell where a mark of another
//! conversion of the book is (#265).
//!
//! A bookmark, highlight or note keeps its places in the conversion it was made
//! in, and the text it is on. In another conversion of the same book those
//! places can mean something else, so a mark of another conversion is drawn only
//! where the page has the text the mark kept: a wrong place is hidden, never
//! shown. Nothing is rewritten.
//!
//! Only the first [`LEAD_MAX`] bytes are compared, white space as one space; a
//! page that ends inside them still matches by what it has (at least
//! [`LEAD_MIN`] bytes), as the mark's text goes on over the page break.

/// Most bytes of a mark's text that are compared (and held for a highlight).
pub const LEAD_MAX: usize = 32;
/// The least bytes that tell a place: a shorter text is on too many pages.
pub const LEAD_MIN: usize = 8;

/// The first [`LEAD_MAX`] bytes of a mark's `text`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lead {
    bytes: [u8; LEAD_MAX],
    len: u8,
}

impl Lead {
    /// The lead of `text`; none where it is too short to tell a place.
    #[must_use]
    pub fn of(text: &str) -> Option<Self> {
        Self::of_any(text).filter(|lead| !lead.is_short())
    }

    /// Whether the text is too short to tell a place by being somewhere near
    /// it: such a mark is told by the very words it covers ([`Lead::says`]).
    #[must_use]
    pub fn is_short(&self) -> bool {
        usize::from(self.len) < LEAD_MIN
    }

    /// Whether `words` (the words a mark covers, white space as one space) are
    /// this text and no more; or its start, word by word, where `open` says the
    /// page ends with them (the mark's text goes on over the page break).
    #[must_use]
    pub fn says(&self, words: &str, open: bool) -> bool {
        let mut want = self.text().chars();
        for got in plain(words) {
            if want.next() != Some(got) {
                return false;
            }
        }
        match want.next() {
            None => true,
            Some(next) => open && next == ' ',
        }
    }

    /// The lead of `text`, however short; none for a text of no letters.
    #[must_use]
    pub fn of_any(text: &str) -> Option<Self> {
        let mut bytes = [0u8; LEAD_MAX];
        let mut len = 0usize;
        for c in plain(text) {
            let n = c.len_utf8();
            if len + n > LEAD_MAX {
                break;
            }
            c.encode_utf8(&mut bytes[len..len + n]);
            len += n;
        }
        // Cut after a word's end: no space left at the end.
        if len > 0 && bytes.get(len - 1) == Some(&b' ') {
            len -= 1;
        }
        (len > 0).then_some(Self {
            bytes,
            len: u8::try_from(len).unwrap_or(0),
        })
    }

    fn text(&self) -> &str {
        self.bytes
            .get(..usize::from(self.len))
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or_default()
    }

    /// Whether `page` has this text: somewhere in it, or at its end as far as
    /// the page goes (at least [`LEAD_MIN`] bytes of it).
    #[must_use]
    pub fn is_in(&self, page: &str) -> bool {
        has(self.text(), page)
    }
}

/// Whether `page` has the start of `stored` (a mark's text) in it.
#[must_use]
pub fn found_in(stored: &str, page: &str) -> bool {
    Lead::of(stored).is_some_and(|lead| lead.is_in(page))
}

fn plain(text: &str) -> impl Iterator<Item = char> + Clone + '_ {
    let mut words = text.split_whitespace();
    let mut first = true;
    let mut word = "".chars();
    let spaced = core::iter::from_fn(move || {
        loop {
            if let Some(c) = word.next() {
                return Some(c);
            }
            let next = words.next()?;
            word = next.chars();
            if first {
                first = false;
            } else {
                return Some(' ');
            }
        }
    });
    // A word broken at a line's end ("any- where") is whole again: both sides
    // of a comparison were broken at other places.
    let mut chars = spaced.peekable();
    let mut before = ' ';
    core::iter::from_fn(move || {
        loop {
            let c = chars.next()?;
            if c == '-' && before.is_alphabetic() && chars.peek() == Some(&' ') {
                chars.next();
                before = c;
                continue;
            }
            before = c;
            return Some(c);
        }
    })
}

fn has(needle: &str, page: &str) -> bool {
    let mut hay = plain(page);
    loop {
        if starts(needle, hay.clone()) {
            return true;
        }
        if hay.next().is_none() {
            return false;
        }
    }
}

/// Whether `hay` begins with `needle`, or ends inside it after at least
/// [`LEAD_MIN`] bytes of it.
fn starts(needle: &str, mut hay: impl Iterator<Item = char>) -> bool {
    let mut matched = 0usize;
    for want in needle.chars() {
        match hay.next() {
            Some(got) if got == want => matched += want.len_utf8(),
            None => return matched >= LEAD_MIN,
            Some(_) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests;
