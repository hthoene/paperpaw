//! The algorithm as a walk through a paragraph, one character at a time.
//!
//! UAX #9 resolves a whole paragraph: explicit levels (X rules), then each
//! isolating run sequence through the weak (W), bracket (N0), neutral
//! (N1, N2) and implicit (I) rules. Here every rule looks back only
//! through what [`Carry`] keeps, and looks ahead by walking a copy of the
//! carry over the rest of the paragraph. So a line is resolved from the
//! carry at its start and the text after it, with the paragraph's result.

use super::{Class, Level, bracket, canonical, class};

/// Embeddings and isolates open at once, the paragraph's own level
/// included. Deeper ones count as overflow.
pub const DEPTH: usize = 8;
/// Isolates with a matching PDI that nest, each keeping the run sequence
/// it interrupts. A deeper one counts as overflow.
pub const ISOLATES: usize = 4;
/// Brackets open at once in a paragraph. One more stops the pairing of
/// brackets for its run sequence, as the 64th does in UAX #9 (BD16).
pub const BRACKETS: usize = 16;
/// Open brackets per run sequence in UAX #9 (BD16).
const BD16_STACK: usize = 63;

/// Flags of a stack entry.
const OVERRIDE_L: u8 = 1;
const OVERRIDE_R: u8 = 2;
const ISOLATE: u8 = 4;
/// An isolate whose PDI exists: its run sequence waits beneath it.
const SUSPENDS: u8 = 8;

/// A bracket's pair type: not paired, paired but left to rules N1 and
/// N2, paired and left-to-right, paired and right-to-left.
const UNPAIRED: u8 = 0;
const PAIRED: u8 = 1;
const PAIRED_L: u8 = 2;
const PAIRED_R: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Entry {
    level: u8,
    flags: u8,
}

/// An open bracket: the closing bracket it waits for, and its pair type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Open {
    close: u16,
    pair: u8,
}

/// What the weak and neutral rules know of an isolating run sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Sequence {
    id: u32,
    level: u8,
    /// The type of the character before, after W1 (rule W1).
    w1: Class,
    /// The last of L, R, AL or sos, after W1 (rule W2).
    w2: Class,
    /// The type of the character before, after W3 (rule W4).
    w3: Class,
    /// Whether the character before is EN after W5 (rule W5).
    w5: bool,
    /// The last of L, R or sos, after W3 (rule W7).
    w7: Class,
    /// The last strong type after N0, numbers as R (rules N0 and N1).
    strong: Class,
    /// The direction of the neutrals being resolved (rule N1), if known.
    neutral: Option<Class>,
    /// The type of the bracket before, for the marks after it (rule N0).
    marks: Option<Class>,
    /// Where its open brackets start in [`Carry::open`].
    brackets: u8,
    /// Whether bracket pairing stopped for it (BD16).
    stopped: bool,
    /// Whether the run of terminators (ET) the last character is in ends
    /// at a European number (rule W5), once looked up: the whole run
    /// shares the answer, so it is looked up once a run.
    et_en: Option<bool>,
}

const NO_SEQUENCE: Sequence = Sequence {
    id: 0,
    level: 0,
    w1: Class::L,
    w2: Class::L,
    w3: Class::L,
    w5: false,
    w7: Class::L,
    strong: Class::L,
    neutral: None,
    marks: None,
    brackets: 0,
    stopped: false,
    et_en: None,
};

/// What the algorithm knows at a point of a paragraph: the state a line
/// starts with. A plain value of fixed size; [`Carry::resolve`] moves it
/// on. Two pieces of one paragraph (a block split in two) share it: the
/// second starts with the carry the first ended with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Carry {
    paragraph: u8,
    stack: [Entry; DEPTH],
    depth: u8,
    overflow_isolates: u16,
    overflow_embeddings: u16,
    valid_isolates: u8,
    /// The level of the last character not removed by X9, if any.
    last: Option<u8>,
    /// The level given to the last character (removed ones take it).
    shown: u8,
    /// A matched isolate opened: the next character starts its content.
    suspend: bool,
    sequences: [Sequence; ISOLATES + 1],
    count: u8,
    open: [Open; BRACKETS],
    brackets: u8,
    /// Wide enough that ids of sequences open at once never meet,
    /// however many a hostile paragraph starts.
    next_id: u32,
}

impl Carry {
    /// The carry at the start of a paragraph at `paragraph`'s level (from
    /// [`paragraph_level`](super::paragraph_level), or the book's
    /// direction).
    #[must_use]
    pub(crate) fn new(paragraph: Level) -> Carry {
        let level = paragraph.0.min(Level::MAX) & 1;
        let base = Entry { level, flags: 0 };
        Carry {
            paragraph: level,
            stack: [base; DEPTH],
            depth: 1,
            overflow_isolates: 0,
            overflow_embeddings: 0,
            valid_isolates: 0,
            last: None,
            shown: level,
            suspend: false,
            sequences: [NO_SEQUENCE; ISOLATES + 1],
            count: 0,
            open: [Open {
                close: 0,
                pair: UNPAIRED,
            }; BRACKETS],
            brackets: 0,
            next_id: 0,
        }
    }

    /// The paragraph's level.
    #[must_use]
    #[allow(dead_code, reason = "for pieces of a paragraph the reader keeps (#33)")]
    pub(crate) fn paragraph(&self) -> Level {
        Level(self.paragraph)
    }

    /// Resolves the next `count` characters of `rest` into `levels` and
    /// moves the carry past them. `rest` runs from the carry's place to
    /// the paragraph's end (all its pieces): what follows decides
    /// neutrals, numbers and brackets near the line's end. Returns how
    /// many levels it wrote: `count`, or fewer where `levels` or `rest`
    /// is shorter. The levels are before rule L1
    /// ([`reset_line_end`](super::reset_line_end)).
    pub(crate) fn resolve<I>(&mut self, rest: I, count: usize, levels: &mut [Level]) -> usize
    where
        I: Iterator<Item = char> + Clone,
    {
        let count = count.min(levels.len());
        let mut walk = Walk { carry: *self, rest };
        let mut written = 0;
        while written < count {
            let Some(level) = walk.level() else {
                break;
            };
            if let Some(slot) = levels.get_mut(written) {
                *slot = Level(level);
            }
            written += 1;
        }
        *self = walk.carry;
        written
    }

    /// Moves the carry past the next `count` characters of `rest`
    /// without keeping their levels: to start a line in the middle of a
    /// paragraph.
    #[allow(dead_code, reason = "for pieces of a paragraph the reader keeps (#33)")]
    pub(crate) fn skip<I>(&mut self, rest: I, count: usize)
    where
        I: Iterator<Item = char> + Clone,
    {
        let mut walk = Walk { carry: *self, rest };
        for _ in 0..count {
            if walk.level().is_none() {
                break;
            }
        }
        *self = walk.carry;
    }

    fn top(&self) -> Entry {
        self.stack[usize::from(self.depth.max(1)) - 1]
    }

    fn sequence(&mut self) -> &mut Sequence {
        let at = usize::from(self.count.max(1)) - 1;
        &mut self.sequences[at]
    }

    fn holds(&self, id: u32) -> bool {
        self.sequences[..usize::from(self.count)]
            .iter()
            .any(|s| s.id == id)
    }

    fn push(&mut self, level: u8, flags: u8) {
        let at = usize::from(self.depth);
        if let Some(slot) = self.stack.get_mut(at) {
            *slot = Entry { level, flags };
            self.depth += 1;
        }
    }

    /// Starts a run sequence at `level` (rule X10), above the current
    /// one when `nested`, else in its place.
    fn start(&mut self, level: u8, nested: bool) {
        let before = self.last.unwrap_or(self.paragraph);
        let sos = parity(before.max(level));
        let brackets = if nested || self.count == 0 {
            self.brackets
        } else {
            self.sequence().brackets
        };
        self.brackets = brackets;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let sequence = Sequence {
            id,
            level,
            w1: sos,
            w2: sos,
            w3: sos,
            w7: sos,
            strong: sos,
            brackets,
            ..NO_SEQUENCE
        };
        if !nested && self.count > 0 {
            *self.sequence() = sequence;
        } else if let Some(slot) = self.sequences.get_mut(usize::from(self.count)) {
            *slot = sequence;
            self.count += 1;
        } else {
            *self.sequence() = sequence;
        }
    }

    /// The run sequence of a character not removed by X9 at `level`
    /// (BD13). `closed` says it is the PDI of an isolate whose run
    /// sequence waits.
    fn enter(&mut self, level: u8, closed: bool) -> u32 {
        if closed {
            if self.suspend {
                self.suspend = false;
            } else if self.count > 1 {
                self.count -= 1;
                self.brackets = self.sequences[usize::from(self.count)].brackets;
            }
        } else if self.suspend {
            self.suspend = false;
            self.start(level, true);
        } else if self.count == 0 || self.last != Some(level) {
            self.start(level, false);
        }
        self.last = Some(level);
        self.sequence().id
    }
}

/// L or R by the parity of `level`.
fn parity(level: u8) -> Class {
    if level & 1 == 1 { Class::R } else { Class::L }
}

/// The strong direction a type counts as for rules N0 and N1.
fn strong(class: Class) -> Option<Class> {
    match class {
        Class::L => Some(Class::L),
        Class::R | Class::AL | Class::EN | Class::AN => Some(Class::R),
        _ => None,
    }
}

/// Whether a type is a neutral or isolate control (BD8, NI).
fn neutral(class: Class) -> bool {
    matches!(
        class,
        Class::B
            | Class::S
            | Class::WS
            | Class::ON
            | Class::LRI
            | Class::RLI
            | Class::FSI
            | Class::PDI
    )
}

fn isolate_control(class: Class) -> bool {
    matches!(class, Class::LRI | Class::RLI | Class::FSI | Class::PDI)
}

/// The level of the first strong character, skipping isolates (P2, P3).
/// With `within`, it stops at the PDI that closes the text (for FSI).
pub(super) fn first_strong(chars: impl Iterator<Item = char>, within: bool) -> Option<Level> {
    let mut depth = 0u32;
    for ch in chars {
        match class(ch) {
            Class::L if depth == 0 => return Some(Level::LTR),
            Class::R | Class::AL if depth == 0 => return Some(Level::RTL),
            Class::LRI | Class::RLI | Class::FSI => depth += 1,
            Class::PDI if depth > 0 => depth -= 1,
            Class::PDI if within => return None,
            Class::B => return None,
            _ => {}
        }
    }
    None
}

/// Whether an isolate initiator has a matching PDI in the paragraph (BD9).
fn has_pdi(chars: impl Iterator<Item = char>) -> bool {
    let mut depth = 0u32;
    for ch in chars {
        match class(ch) {
            Class::LRI | Class::RLI | Class::FSI => depth += 1,
            Class::PDI if depth == 0 => return true,
            Class::PDI => depth -= 1,
            Class::B => return false,
            _ => {}
        }
    }
    false
}

/// A character after the explicit rules.
#[derive(Clone, Copy, Debug)]
struct Explicit {
    ch: char,
    /// Its type after X rules (overrides applied).
    class: Class,
    level: u8,
    removed: bool,
    sequence: u32,
    /// An isolate initiator without a matching PDI (rule X10's eos).
    unmatched: bool,
}

/// A character after the weak rules and N0.
#[derive(Clone, Copy, Debug)]
struct Resolved {
    x: Explicit,
    /// After the weak rules and N0.
    paired: Class,
}

#[derive(Clone)]
struct Walk<I> {
    carry: Carry,
    rest: I,
}

impl<I: Iterator<Item = char> + Clone> Walk<I> {
    /// The next character through the explicit rules (X1 to X10).
    fn explicit(&mut self) -> Option<Explicit> {
        let ch = self.rest.next()?;
        let original = class(ch);
        let top = self.carry.top();
        let mut x = Explicit {
            ch,
            class: overridden(top, original),
            level: top.level,
            removed: false,
            sequence: 0,
            unmatched: false,
        };
        let mut closed = false;
        let mut opens = false;
        match original {
            Class::RLE | Class::LRE | Class::RLO | Class::LRO => {
                self.embed(original);
                x.removed = true;
            }
            Class::RLI | Class::LRI | Class::FSI => {
                let (matched, valid) = self.isolate(original);
                x.unmatched = !matched;
                opens = matched && valid;
            }
            Class::PDI => {
                closed = self.close_isolate();
                let top = self.carry.top();
                x.level = top.level;
                x.class = overridden(top, original);
            }
            Class::PDF => {
                self.pop_embedding();
                x.removed = true;
            }
            Class::B => {
                x.level = self.carry.paragraph;
                x.class = Class::B;
                self.carry.depth = 1;
                self.carry.overflow_isolates = 0;
                self.carry.overflow_embeddings = 0;
                self.carry.valid_isolates = 0;
            }
            Class::BN => x.removed = true,
            _ => {}
        }
        if x.removed {
            x.level = self.carry.shown;
            x.sequence = self.carry.sequences[usize::from(self.carry.count.max(1)) - 1].id;
            return Some(x);
        }
        x.sequence = self.carry.enter(x.level, closed);
        if opens {
            self.carry.suspend = true;
        }
        Some(x)
    }

    /// Rules X2 to X5: an embedding or override.
    fn embed(&mut self, class: Class) {
        let c = &mut self.carry;
        let top = c.top();
        let rtl = matches!(class, Class::RLE | Class::RLO);
        let level = next_level(top.level, rtl);
        let flags = match class {
            Class::RLO => OVERRIDE_R,
            Class::LRO => OVERRIDE_L,
            _ => 0,
        };
        if level <= Level::MAX
            && c.overflow_isolates == 0
            && c.overflow_embeddings == 0
            && usize::from(c.depth) < DEPTH
        {
            c.push(level, flags);
        } else if c.overflow_isolates == 0 {
            c.overflow_embeddings = c.overflow_embeddings.saturating_add(1);
        }
    }

    /// Rules X5a to X5c: an isolate. Returns whether it has a matching
    /// PDI and whether it is valid.
    fn isolate(&mut self, class: Class) -> (bool, bool) {
        // An isolate that overflows opens no level, so neither its
        // direction nor its PDI matters. Not looking for them keeps the
        // look-ahead linear: each valid isolate looks to its PDI (or the
        // paragraph's end) and stays open until there, and at most
        // `DEPTH` are open at once.
        let c = &self.carry;
        if c.overflow_isolates > 0 || c.overflow_embeddings > 0 || usize::from(c.depth) >= DEPTH {
            self.carry.overflow_isolates = self.carry.overflow_isolates.saturating_add(1);
            // Taken as matched: its content stays at its level, so no
            // run sequence ends at it (X10) either way.
            return (true, false);
        }
        let rtl = match class {
            Class::RLI => true,
            Class::FSI => first_strong(self.rest.clone(), true) == Some(Level::RTL),
            _ => false,
        };
        let matched = has_pdi(self.rest.clone());
        let c = &mut self.carry;
        let level = next_level(c.top().level, rtl);
        let room = !matched || usize::from(c.count) <= ISOLATES;
        let valid = level <= Level::MAX
            && c.overflow_isolates == 0
            && c.overflow_embeddings == 0
            && usize::from(c.depth) < DEPTH
            && room;
        if valid {
            c.valid_isolates += 1;
            c.push(level, ISOLATE | if matched { SUSPENDS } else { 0 });
        } else {
            c.overflow_isolates = c.overflow_isolates.saturating_add(1);
        }
        (matched, valid)
    }

    /// Rule X6a: a PDI. Returns whether it closed an isolate whose run
    /// sequence waits.
    fn close_isolate(&mut self) -> bool {
        let c = &mut self.carry;
        if c.overflow_isolates > 0 {
            c.overflow_isolates -= 1;
            return false;
        }
        if c.valid_isolates == 0 {
            return false;
        }
        c.overflow_embeddings = 0;
        while c.depth > 1 && c.top().flags & ISOLATE == 0 {
            c.depth -= 1;
        }
        let entry = c.top();
        if c.depth > 1 {
            c.depth -= 1;
        }
        c.valid_isolates -= 1;
        entry.flags & SUSPENDS != 0
    }

    /// Rule X7: a PDF.
    fn pop_embedding(&mut self) {
        let c = &mut self.carry;
        if c.overflow_isolates > 0 {
        } else if c.overflow_embeddings > 0 {
            c.overflow_embeddings -= 1;
        } else if c.top().flags & ISOLATE == 0 && c.depth >= 2 {
            c.depth -= 1;
        }
    }

    /// The next character through W1 to W3, with the type after W3.
    fn early_weak(&mut self) -> Option<(Explicit, Class)> {
        let x = self.explicit()?;
        if x.removed {
            return Some((x, Class::BN));
        }
        let s = self.carry.sequence();
        let mut t = x.class;
        if t == Class::NSM {
            t = if isolate_control(s.w1) {
                Class::ON
            } else {
                s.w1
            };
        }
        s.w1 = t;
        if matches!(t, Class::L | Class::R | Class::AL) {
            s.w2 = t;
        } else if t == Class::EN && s.w2 == Class::AL {
            t = Class::AN;
        }
        if t == Class::AL {
            t = Class::R;
        }
        Some((x, t))
    }

    /// The type after W3 of the next character of run sequence `id`.
    /// With `past_et`, terminators (ET) are passed over first.
    fn peek(&self, id: u32, past_et: bool) -> Option<Class> {
        let mut ahead = self.clone();
        loop {
            let (x, t) = ahead.early_weak()?;
            if x.removed {
                continue;
            }
            if x.sequence != id {
                if ahead.carry.holds(id) {
                    continue;
                }
                return None;
            }
            if past_et && t == Class::ET {
                continue;
            }
            return Some(t);
        }
    }

    /// The next character through all weak rules (W1 to W7).
    fn weak(&mut self) -> Option<(Explicit, Class)> {
        let (x, mut t) = self.early_weak()?;
        if x.removed {
            return Some((x, t));
        }
        let (id, w3, w5) = {
            let s = self.carry.sequence();
            (s.id, s.w3, s.w5)
        };
        let early = t;
        // W4: one separator between two numbers of a kind.
        if matches!(t, Class::ES | Class::CS) && matches!(w3, Class::EN | Class::AN) {
            let next = self.peek(id, false);
            if next == Some(w3) && (t == Class::CS || w3 == Class::EN) {
                t = w3;
            }
        }
        // W5: terminators next to a European number.
        if t == Class::ET {
            let known = self.carry.sequence().et_en;
            let en = w5
                || known.unwrap_or_else(|| {
                    let en = self.peek(id, true) == Some(Class::EN);
                    let sequence = self.carry.sequence();
                    sequence.et_en = Some(en);
                    en
                });
            if en {
                t = Class::EN;
            }
        } else {
            let sequence = self.carry.sequence();
            sequence.et_en = None;
        }
        // W6: other separators and terminators.
        if matches!(t, Class::ES | Class::ET | Class::CS) {
            t = Class::ON;
        }
        let s = self.carry.sequence();
        s.w3 = early;
        s.w5 = t == Class::EN;
        // W7: European numbers after left-to-right text.
        if t == Class::EN && s.w7 == Class::L {
            t = Class::L;
        }
        if matches!(early, Class::L | Class::R) {
            s.w7 = early;
        }
        Some((x, t))
    }

    /// The next character through the weak rules and N0.
    fn pairs(&mut self) -> Option<Resolved> {
        let (x, weak) = self.weak()?;
        let mut r = Resolved { x, paired: weak };
        if x.removed {
            return Some(r);
        }
        let mut bracket_type = None;
        if weak == Class::ON
            && let Some((pair, open)) = bracket(x.ch)
        {
            bracket_type = if open {
                self.open_bracket(pair)
            } else {
                self.close_bracket(x.ch)
            };
        }
        let s = self.carry.sequence();
        if let Some(t) = bracket_type {
            r.paired = t;
            s.marks = Some(t);
        } else if x.class == Class::NSM && s.marks.is_some() {
            r.paired = s.marks.unwrap_or(Class::ON);
        } else {
            s.marks = None;
        }
        if let Some(d) = strong(r.paired) {
            s.strong = d;
        }
        Some(r)
    }

    /// An opening bracket (BD16, N0): pushes it and returns the type of
    /// its pair, if it pairs and takes a direction.
    fn open_bracket(&mut self, close: u16) -> Option<Class> {
        let (stopped, base) = {
            let s = self.carry.sequence();
            (s.stopped, usize::from(s.brackets))
        };
        if stopped {
            return None;
        }
        let at = usize::from(self.carry.brackets);
        if at >= BRACKETS || at.checked_sub(base).is_none_or(|n| n >= BD16_STACK) {
            let sequence = self.carry.sequence();
            sequence.stopped = true;
            return None;
        }
        let pair = self.pair_of(close, base, at);
        *self.carry.open.get_mut(at)? = Open { close, pair };
        self.carry.brackets += 1;
        match pair {
            PAIRED_L => Some(Class::L),
            PAIRED_R => Some(Class::R),
            _ => None,
        }
    }

    /// A closing bracket: pops to its opening bracket, if one waits.
    fn close_bracket(&mut self, ch: char) -> Option<Class> {
        let s = *self.carry.sequence();
        if s.stopped {
            return None;
        }
        let cp = canonical(u16::try_from(u32::from(ch)).ok()?);
        let base = usize::from(s.brackets);
        let top = usize::from(self.carry.brackets);
        let open = self.carry.open.get(base..top)?;
        let found = open.iter().rposition(|o| o.close == cp)?;
        let pair = open.get(found)?.pair;
        self.carry.brackets = u8::try_from(base + found).unwrap_or(0);
        match pair {
            PAIRED_L => Some(Class::L),
            PAIRED_R => Some(Class::R),
            _ => None,
        }
    }

    /// Whether the bracket about to open at `at` finds its pair (BD16),
    /// and the pair's type by the strong types inside it and before it
    /// (N0). Walks a copy ahead.
    fn pair_of(&self, close: u16, base: usize, at: usize) -> u8 {
        let s = self.carry.sequences[usize::from(self.carry.count.max(1)) - 1];
        let embedding = parity(s.level);
        let mut stack = [0u16; BRACKETS + 1];
        let Some(open) = self.carry.open.get(base..at) else {
            return UNPAIRED;
        };
        for (slot, open) in stack.iter_mut().zip(open) {
            *slot = open.close;
        }
        let mine = at - base;
        let Some(slot) = stack.get_mut(mine) else {
            return UNPAIRED;
        };
        *slot = close;
        let mut len = mine + 1;
        let limit = BRACKETS.saturating_sub(base).min(BD16_STACK);
        let (mut same, mut opposite) = (false, false);
        let mut ahead = self.clone();
        loop {
            let Some((x, t)) = ahead.weak() else {
                return UNPAIRED;
            };
            if x.removed {
                continue;
            }
            if x.sequence != s.id {
                if ahead.carry.holds(s.id) {
                    continue;
                }
                return UNPAIRED;
            }
            if let Some(d) = strong(t) {
                if d == embedding {
                    same = true;
                } else {
                    opposite = true;
                }
            }
            if t != Class::ON {
                continue;
            }
            let Some((pair, open)) = bracket(x.ch) else {
                continue;
            };
            if open {
                let Some(slot) = stack.get_mut(len).filter(|_| len < limit) else {
                    return UNPAIRED;
                };
                *slot = pair;
                len += 1;
                continue;
            }
            let Ok(cp) = u16::try_from(u32::from(x.ch)) else {
                continue;
            };
            let cp = canonical(cp);
            if let Some(k) = stack
                .get(..len)
                .and_then(|s| s.iter().rposition(|&c| c == cp))
            {
                if k == mine {
                    break;
                }
                if k < mine {
                    return UNPAIRED;
                }
                len = k;
            }
        }
        let direction = if same {
            embedding
        } else if opposite {
            if s.strong == embedding {
                embedding
            } else {
                opposite_of(embedding)
            }
        } else {
            return PAIRED;
        };
        if direction == Class::L {
            PAIRED_L
        } else {
            PAIRED_R
        }
    }

    /// The next character's level (rules N1, N2, I1, I2).
    fn level(&mut self) -> Option<u8> {
        let r = self.pairs()?;
        if r.x.removed {
            return Some(self.carry.shown);
        }
        let s = *self.carry.sequence();
        let t = if neutral(r.paired) {
            let d = match s.neutral {
                Some(d) => d,
                None => self.neutrals(&r, &s),
            };
            let sequence = self.carry.sequence();
            sequence.neutral = Some(d);
            d
        } else {
            let sequence = self.carry.sequence();
            sequence.neutral = None;
            r.paired
        };
        let level = implicit(s.level, t);
        self.carry.shown = level;
        Some(level)
    }

    /// The direction of the neutrals that start with `r` (N1, N2): from
    /// the strong types on both sides, else the embedding's.
    fn neutrals(&self, r: &Resolved, s: &Sequence) -> Class {
        let embedding = parity(s.level);
        let mut ahead = self.clone();
        let mut unmatched = r.x.unmatched;
        let after = loop {
            let Some(next) = ahead.pairs() else {
                break parity(s.level.max(self.carry.paragraph));
            };
            if next.x.removed {
                continue;
            }
            if next.x.sequence != s.id {
                if ahead.carry.holds(s.id) {
                    continue;
                }
                let other = if unmatched {
                    self.carry.paragraph
                } else {
                    next.x.level
                };
                break parity(s.level.max(other));
            }
            if neutral(next.paired) {
                unmatched = next.x.unmatched;
                continue;
            }
            break strong(next.paired).unwrap_or(embedding);
        };
        if after == s.strong { after } else { embedding }
    }
}

/// Rules I1 and I2.
fn implicit(level: u8, class: Class) -> u8 {
    match (level & 1 == 1, class) {
        (false, Class::R) => level + 1,
        (false, Class::AN | Class::EN) => level + 2,
        (true, Class::L | Class::EN | Class::AN) => level + 1,
        _ => level,
    }
}

fn opposite_of(class: Class) -> Class {
    if class == Class::L {
        Class::R
    } else {
        Class::L
    }
}

fn next_level(level: u8, rtl: bool) -> u8 {
    if rtl {
        (level + 1) | 1
    } else {
        (level + 2) & !1
    }
}

fn overridden(top: Entry, class: Class) -> Class {
    if top.flags & OVERRIDE_L != 0 {
        Class::L
    } else if top.flags & OVERRIDE_R != 0 {
        Class::R
    } else {
        class
    }
}
