//! A [`Carry`] as bytes: what a reader keeps of a paragraph between its
//! pieces (#33). The bytes are the algorithm's own, not part of the
//! plugin interface (ADR 0026): their size is [`LEN`], told by a function
//! rather than a constant, and bytes of another build or of nothing (a
//! wrong format, sizes out of range) read as none, never as a panic.

use super::{BRACKETS, Carry, DEPTH, Entry, ISOLATES, Open, Sequence};
use crate::bidi::{CLASSES, Class, Level};

/// The format of the bytes, first among them.
const FORMAT: u8 = 1;
/// Bytes of an encoded sequence.
const SEQUENCE: usize = 16;
/// Bytes of an encoded carry.
pub(crate) const LEN: usize = 1
    + 1
    + 2 * DEPTH
    + 1
    + 2
    + 2
    + 1
    + 2
    + 1
    + 1
    + SEQUENCE * (ISOLATES + 1)
    + 1
    + 3 * BRACKETS
    + 1
    + 4;
/// `None` of a class.
const NO_CLASS: u8 = 0xFF;

struct Writer<'a> {
    out: &'a mut [u8],
    at: usize,
}

impl Writer<'_> {
    fn u8(&mut self, value: u8) {
        if let Some(slot) = self.out.get_mut(self.at) {
            *slot = value;
        }
        self.at += 1;
    }

    fn u16(&mut self, value: u16) {
        for b in value.to_le_bytes() {
            self.u8(b);
        }
    }

    fn u32(&mut self, value: u32) {
        for b in value.to_le_bytes() {
            self.u8(b);
        }
    }

    fn class(&mut self, class: Option<Class>) {
        let index = class
            .and_then(|c| CLASSES.iter().position(|k| *k == c))
            .and_then(|i| u8::try_from(i).ok());
        self.u8(index.unwrap_or(NO_CLASS));
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    /// A byte no larger than `most`.
    fn upto(&mut self, most: usize) -> Option<u8> {
        self.u8().filter(|b| usize::from(*b) <= most)
    }

    fn flag(&mut self) -> Option<bool> {
        Some(self.upto(1)? == 1)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes([
            self.u8()?,
            self.u8()?,
            self.u8()?,
            self.u8()?,
        ]))
    }

    fn level(&mut self) -> Option<u8> {
        self.upto(usize::from(Level::MAX) + 1)
    }

    /// A class, or `None` written as such where `optional`.
    fn class(&mut self, optional: bool) -> Option<Option<Class>> {
        match self.u8()? {
            NO_CLASS if optional => Some(None),
            i => CLASSES.get(usize::from(i)).copied().map(Some),
        }
    }

    fn some_class(&mut self) -> Option<Class> {
        self.class(false)?
    }
}

impl Carry {
    /// Writes the carry into `out`, at least [`LEN`] long; false where it
    /// is shorter (nothing written then).
    pub(crate) fn encode(&self, out: &mut [u8]) -> bool {
        if out.len() < LEN {
            return false;
        }
        let mut w = Writer { out, at: 0 };
        w.u8(FORMAT);
        w.u8(self.paragraph);
        for e in &self.stack {
            w.u8(e.level);
            w.u8(e.flags);
        }
        w.u8(self.depth);
        w.u16(self.overflow_isolates);
        w.u16(self.overflow_embeddings);
        w.u8(self.valid_isolates);
        w.u8(u8::from(self.last.is_some()));
        w.u8(self.last.unwrap_or(0));
        w.u8(self.shown);
        w.u8(u8::from(self.suspend));
        for s in &self.sequences {
            w.u32(s.id);
            w.u8(s.level);
            for class in [s.w1, s.w2, s.w3] {
                w.class(Some(class));
            }
            w.u8(u8::from(s.w5));
            w.class(Some(s.w7));
            w.class(Some(s.strong));
            w.class(s.neutral);
            w.class(s.marks);
            w.u8(s.brackets);
            w.u8(u8::from(s.stopped));
            w.u8(s.et_en.map_or(2, u8::from));
        }
        w.u8(self.count);
        for o in &self.open {
            w.u16(o.close);
            w.u8(o.pair);
        }
        w.u8(self.brackets);
        w.u32(self.next_id);
        true
    }

    /// The carry `bytes` hold, as [`Carry::encode`] wrote it; none where
    /// they are not one.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Carry> {
        let mut r = Reader { bytes, at: 0 };
        if r.u8()? != FORMAT {
            return None;
        }
        let mut c = Carry::new(Level::LTR);
        c.paragraph = r.upto(1)?;
        for e in &mut c.stack {
            *e = Entry {
                level: r.level()?,
                flags: r.upto(15)?,
            };
        }
        c.depth = r.upto(DEPTH).filter(|d| *d >= 1)?;
        c.overflow_isolates = r.u16()?;
        c.overflow_embeddings = r.u16()?;
        c.valid_isolates = r.upto(DEPTH)?;
        let has_last = r.flag()?;
        let last = r.level()?;
        c.last = has_last.then_some(last);
        c.shown = r.level()?;
        c.suspend = r.flag()?;
        for s in &mut c.sequences {
            *s = Sequence {
                id: r.u32()?,
                level: r.level()?,
                w1: r.some_class()?,
                w2: r.some_class()?,
                w3: r.some_class()?,
                w5: r.flag()?,
                w7: r.some_class()?,
                strong: r.some_class()?,
                neutral: r.class(true)?,
                marks: r.class(true)?,
                brackets: r.upto(BRACKETS)?,
                stopped: r.flag()?,
                et_en: match r.upto(2)? {
                    2 => None,
                    b => Some(b == 1),
                },
            };
        }
        c.count = r.upto(ISOLATES + 1)?;
        for o in &mut c.open {
            *o = Open {
                close: r.u16()?,
                pair: r.upto(3)?,
            };
        }
        c.brackets = r.upto(BRACKETS)?;
        c.next_id = r.u32()?;
        Some(c)
    }
}
