//! The line-break rules of UAX #14 (LB1 to LB31 of the revision named in
//! `table_data.rs`'s Unicode version), as a small state machine fed one
//! character at a time.
//!
//! `feed(c, rest)` says whether a line may break **before** `c`; `rest` is
//! the text after `c` (a few rules look ahead: LB15b, LB19a, LB25, LB28a).
//! The state is `Copy` and a few bytes, so the greedy wrapper can rebuild it
//! at a line start by feeding the line again (design, section 5.4).
//!
//! Two profiles. The **full** one is UAX #14 (the conformance test of
//! `LineBreakTest.txt`). The **compat** one (what the wrappers run) lets
//! UAX #14 decide a position only when a character next to it is flagged in
//! the table (CJK, Hangul, Thai and the like, U+200B); everywhere else it
//! gives what the wrappers did before: a break at a run of spaces, and after
//! `-` U+2010 U+2013 U+2014 when the next character is not a space. So a text
//! without a flagged character is laid out as before (design, section 6).

use super::table::{Entry, lookup};
use super::table_data::Class;

/// The bits of `paperpaw_layout::ScriptOptions` this module reads (the same
/// positions; a test in `paperpaw-layout` keeps them equal by behaviour).
pub(crate) const KEEP_ALL: u32 = 1 << 2;
pub(crate) const LINE_BREAK_MASK: u32 = 3 << 3;
pub(crate) const LINE_BREAK_LOOSE: u32 = 1 << 3;
pub(crate) const LINE_BREAK_ANYWHERE: u32 = 3 << 3;
pub(crate) const BREAK_ALL: u32 = 1 << 5;

/// Whether a line may break before a character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Opportunity {
    No,
    Allowed,
    Mandatory,
}

/// U+25CC DOTTED CIRCLE, a member of the Aksara group of LB28a.
const DOTTED_CIRCLE: char = '\u{25CC}';

/// A class as the rules see it: after the tailoring, with what the rules
/// ask beside the class.
#[derive(Clone, Copy)]
struct Seen {
    class: Class,
    wide: bool,
    /// An unassigned pictograph (LB30b).
    pictograph: bool,
    circle: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LineBreaker {
    options: u32,
    /// The compat profile (the wrappers) or the full one (the conformance
    /// test).
    compat: bool,
    started: bool,
    /// The class of the last base character, `SP` in a run of spaces.
    prev: Class,
    /// The class before the run of spaces.
    pre_space: Class,
    prev_wide: bool,
    prev_pictograph: bool,
    /// The last character was a zero width joiner.
    zwj: bool,
    /// An odd number of regional indicators so far in this run.
    ri_odd: bool,
    /// LB25: 1 after `NU (SY | IS)*`, 2 after that and `CL` or `CP`.
    number: u8,
    /// LB28a: 1 after `AK | ◌ | AS`, 2 after that and `VI`.
    aksara: u8,
    /// `HL (HY | HH)` just read (LB21a).
    hyphen_after_hebrew: bool,
    /// A word-initial `HY | HH` just read (LB20a).
    hyphen_initial: bool,
    /// An initial quotation mark (and spaces) just read (LB15a).
    quote_initial: bool,
    /// A quotation mark after a non-East Asian character or at the start
    /// was just read (LB19a).
    quote_after_narrow: bool,
    // The compat profile's own memory.
    /// The last character that is not a space is flagged.
    last_flagged: bool,
    /// ... is `-` U+2010 U+2013 or U+2014.
    last_dash: bool,
    /// A space came since it.
    spaces: bool,
}

impl LineBreaker {
    /// The breaker the wrappers run (the compat profile).
    pub(crate) const fn new(options: u32) -> Self {
        Self::make(options, true)
    }

    /// Full UAX #14, for the conformance test.
    #[cfg(test)]
    pub(crate) const fn full() -> Self {
        Self::make(0, false)
    }

    const fn make(options: u32, compat: bool) -> Self {
        LineBreaker {
            options,
            compat,
            started: false,
            prev: Class::AL,
            pre_space: Class::AL,
            prev_wide: false,
            prev_pictograph: false,
            zwj: false,
            ri_odd: false,
            number: 0,
            aksara: 0,
            hyphen_after_hebrew: false,
            hyphen_initial: false,
            quote_initial: false,
            quote_after_narrow: false,
            last_flagged: false,
            last_dash: false,
            spaces: false,
        }
    }

    /// Whether the table flags `c`: UAX #14 decides near it.
    pub(crate) fn flagged(c: char) -> bool {
        lookup(c).uax
    }

    fn seen(&self, entry: Entry) -> Seen {
        let mut class = entry.class;
        let mut pictograph = false;
        if matches!(class, Class::IDEP | Class::ALEP) {
            // An unassigned pictograph has the class it has (ID, or AL as
            // XX does) and does not break before an emoji modifier (LB30b).
            class = if class == Class::IDEP {
                Class::ID
            } else {
                Class::AL
            };
            pictograph = true;
        }
        if self.options & KEEP_ALL != 0
            && matches!(
                class,
                Class::ID | Class::H2 | Class::H3 | Class::JL | Class::JV | Class::JT
            )
        {
            class = Class::AL;
        }
        if self.options & BREAK_ALL != 0 && matches!(class, Class::AL | Class::HL | Class::NU) {
            class = Class::ID;
        }
        if class == Class::CJ {
            class = if self.options & LINE_BREAK_MASK == LINE_BREAK_LOOSE {
                Class::ID
            } else {
                Class::NS
            };
        }
        Seen {
            class,
            wide: entry.wide,
            pictograph,
            circle: false,
        }
    }

    /// The `n`th base character after the current one, skipping the
    /// combining marks and joiners that attach to what is before them (LB9).
    fn base_after(&self, rest: &str, n: usize) -> Option<Seen> {
        let mut found = 0;
        for c in rest.chars() {
            let seen = Seen {
                circle: c == DOTTED_CIRCLE,
                ..self.seen(lookup(c))
            };
            if matches!(seen.class, Class::CM | Class::ZWJ) {
                continue;
            }
            if found == n {
                return Some(seen);
            }
            found += 1;
        }
        None
    }

    /// Feeds `c`, the next character, and says whether a line may (or must)
    /// break before it. `rest` is the text after `c`.
    pub(crate) fn feed(&mut self, c: char, rest: &str) -> Opportunity {
        let entry = lookup(c);
        let mut now = Seen {
            circle: c == DOTTED_CIRCLE,
            ..self.seen(entry)
        };
        let before = *self;
        let uax = self.uax(&mut now, rest);
        // The compat profile: UAX #14 decides where a flagged character is
        // next to the position (or the options lift the filter); elsewhere
        // the wrappers' old rule decides.
        let decided = if !before.started {
            Opportunity::No
        } else if !before.compat
            || before.last_flagged
            || entry.uax
            || before.options & LINE_BREAK_MASK == LINE_BREAK_ANYWHERE
            || before.options & BREAK_ALL != 0
        {
            uax
        } else if c != ' ' && (before.spaces || before.last_dash) {
            Opportunity::Allowed
        } else {
            Opportunity::No
        };
        if c == ' ' {
            self.spaces = true;
        } else {
            self.last_flagged = entry.uax;
            self.last_dash = matches!(c, '-' | '\u{2010}' | '\u{2013}' | '\u{2014}');
            self.spaces = false;
        }
        self.started = true;
        decided
    }

    /// UAX #14 for the position before `now`, and the state after it.
    fn uax(&mut self, now: &mut Seen, rest: &str) -> Opportunity {
        use Opportunity::{Allowed, Mandatory, No};
        let sot = !self.started;
        let was = *self;
        let prev = was.prev;
        let in_spaces = prev == Class::SP;
        // The class before a run of spaces (the rules with `SP*`).
        let base = if in_spaces { was.pre_space } else { prev };
        let hard = |class: Class| matches!(class, Class::BK | Class::CR | Class::LF | Class::NL);
        let attaches = matches!(now.class, Class::CM | Class::ZWJ)
            && !sot
            && !hard(prev)
            && prev != Class::SP
            && prev != Class::ZW;
        let is_zwj = now.class == Class::ZWJ;
        // LB10: a stray mark is a letter.
        if matches!(now.class, Class::CM | Class::ZWJ) && !attaches {
            now.class = Class::AL;
        }
        let c = now.class;

        let result = 'rules: {
            // LB2
            if sot {
                break 'rules No;
            }
            // LB4, LB5, LB6
            if matches!(prev, Class::BK | Class::LF | Class::NL)
                || (prev == Class::CR && c != Class::LF)
            {
                break 'rules Mandatory;
            }
            if prev == Class::CR && c == Class::LF {
                break 'rules No;
            }
            if hard(c) {
                break 'rules No;
            }
            // LB7
            if c == Class::SP || c == Class::ZW {
                break 'rules No;
            }
            // LB8
            if prev == Class::ZW || (in_spaces && was.pre_space == Class::ZW) {
                break 'rules Allowed;
            }
            // LB8a
            if was.zwj {
                break 'rules No;
            }
            // LB9
            if attaches {
                break 'rules No;
            }
            // LB11
            if c == Class::WJ || prev == Class::WJ {
                break 'rules No;
            }
            // Tailoring: a line may end between any two clusters.
            if self.options & LINE_BREAK_MASK == LINE_BREAK_ANYWHERE {
                break 'rules Allowed;
            }
            // LB12, LB12a
            if prev == Class::GL {
                break 'rules No;
            }
            // (LineBreakTest.txt of 17.0.0 also breaks after BA: `[^SP BA HY HH] × GL`.)
            if c == Class::GL && !matches!(prev, Class::SP | Class::BA | Class::HY | Class::HH) {
                break 'rules No;
            }
            // LB13
            if matches!(c, Class::CL | Class::CP | Class::EX | Class::SY) {
                break 'rules No;
            }
            // LB14
            if base == Class::OP {
                break 'rules No;
            }
            // LB15a
            if was.quote_initial {
                break 'rules No;
            }
            // LB15b: a final quotation mark before a space, a closing mark
            // or the end.
            if c == Class::QUPF {
                match self.base_after(rest, 0) {
                    None => break 'rules No,
                    Some(next)
                        if matches!(
                            next.class,
                            Class::SP
                                | Class::GL
                                | Class::WJ
                                | Class::CL
                                | Class::QU
                                | Class::QUPI
                                | Class::QUPF
                                | Class::CP
                                | Class::EX
                                | Class::IS
                                | Class::SY
                                | Class::BK
                                | Class::CR
                                | Class::LF
                                | Class::NL
                                | Class::ZW
                        ) =>
                    {
                        break 'rules No;
                    }
                    Some(_) => {}
                }
            }
            // LB15c, LB15d
            if in_spaces
                && c == Class::IS
                && self
                    .base_after(rest, 0)
                    .is_some_and(|n| n.class == Class::NU)
            {
                break 'rules Allowed;
            }
            if c == Class::IS {
                break 'rules No;
            }
            // LB16
            if matches!(base, Class::CL | Class::CP) && c == Class::NS {
                break 'rules No;
            }
            // LB17
            if base == Class::B2 && c == Class::B2 {
                break 'rules No;
            }
            // LB18
            if in_spaces {
                break 'rules Allowed;
            }
            let quote = |k: Class| matches!(k, Class::QU | Class::QUPI | Class::QUPF);
            // LB19
            if matches!(c, Class::QU | Class::QUPF) || matches!(prev, Class::QU | Class::QUPI) {
                break 'rules No;
            }
            // LB19a
            if quote(c) && !was.prev_wide {
                break 'rules No;
            }
            if quote(c) && self.base_after(rest, 0).is_none_or(|n| !n.wide) {
                break 'rules No;
            }
            if quote(prev) && !now.wide {
                break 'rules No;
            }
            if was.quote_after_narrow {
                break 'rules No;
            }
            // LB20
            if c == Class::CB || prev == Class::CB {
                break 'rules Allowed;
            }
            // LB20a
            if was.hyphen_initial && matches!(c, Class::AL | Class::HL) {
                break 'rules No;
            }
            // LB21, LB21a, LB21b
            if matches!(c, Class::BA | Class::HH | Class::HY | Class::NS) || prev == Class::BB {
                break 'rules No;
            }
            if was.hyphen_after_hebrew && c != Class::HL {
                break 'rules No;
            }
            if prev == Class::SY && c == Class::HL {
                break 'rules No;
            }
            // LB22
            if c == Class::IN {
                break 'rules No;
            }
            // LB23, LB23a, LB24
            let letter = |k: Class| matches!(k, Class::AL | Class::HL);
            if (letter(prev) && c == Class::NU) || (prev == Class::NU && letter(c)) {
                break 'rules No;
            }
            let pictogram = |k: Class| matches!(k, Class::ID | Class::EB | Class::EM);
            if (prev == Class::PR && pictogram(c)) || (pictogram(prev) && c == Class::PO) {
                break 'rules No;
            }
            if (matches!(prev, Class::PR | Class::PO) && letter(c))
                || (letter(prev) && matches!(c, Class::PR | Class::PO))
            {
                break 'rules No;
            }
            // LB25
            if matches!(c, Class::PO | Class::PR) && was.number != 0 {
                break 'rules No;
            }
            if matches!(prev, Class::PO | Class::PR) {
                if c == Class::NU {
                    break 'rules No;
                }
                if c == Class::OP {
                    let first = self.base_after(rest, 0);
                    let second = self.base_after(rest, 1);
                    if first.is_some_and(|n| n.class == Class::NU)
                        || (first.is_some_and(|n| n.class == Class::IS)
                            && second.is_some_and(|n| n.class == Class::NU))
                    {
                        break 'rules No;
                    }
                }
            }
            if (prev == Class::HY || prev == Class::IS) && c == Class::NU {
                break 'rules No;
            }
            if was.number == 1 && c == Class::NU {
                break 'rules No;
            }
            // LB26, LB27
            let jamo =
                |k: Class| matches!(k, Class::JL | Class::JV | Class::JT | Class::H2 | Class::H3);
            if (prev == Class::JL && matches!(c, Class::JL | Class::JV | Class::H2 | Class::H3))
                || (matches!(prev, Class::JV | Class::H2) && matches!(c, Class::JV | Class::JT))
                || (matches!(prev, Class::JT | Class::H3) && c == Class::JT)
            {
                break 'rules No;
            }
            if (jamo(prev) && c == Class::PO) || (prev == Class::PR && jamo(c)) {
                break 'rules No;
            }
            // LB28
            if letter(prev) && letter(c) {
                break 'rules No;
            }
            // LB28a
            let member = |s: Seen| matches!(s.class, Class::AK | Class::AS) || s.circle;
            if prev == Class::AP && member(*now) {
                break 'rules No;
            }
            if was.aksara == 1 && matches!(c, Class::VF | Class::VI) {
                break 'rules No;
            }
            if was.aksara == 2 && (c == Class::AK || now.circle) {
                break 'rules No;
            }
            if was.aksara == 1
                && member(*now)
                && self
                    .base_after(rest, 0)
                    .is_some_and(|n| n.class == Class::VF)
            {
                break 'rules No;
            }
            // LB29
            if prev == Class::IS && letter(c) {
                break 'rules No;
            }
            // LB30
            if (letter(prev) || prev == Class::NU) && c == Class::OP && !now.wide {
                break 'rules No;
            }
            if prev == Class::CP && !was.prev_wide && (letter(c) || c == Class::NU) {
                break 'rules No;
            }
            // LB30a
            if prev == Class::RI && c == Class::RI && was.ri_odd {
                break 'rules No;
            }
            // LB30b
            if c == Class::EM && (prev == Class::EB || was.prev_pictograph) {
                break 'rules No;
            }
            // LB31
            Allowed
        };

        // The state after `now`.
        self.zwj = is_zwj;
        if attaches {
            return result;
        }
        let ctx = |k: Class| {
            matches!(
                k,
                Class::BK
                    | Class::CR
                    | Class::LF
                    | Class::NL
                    | Class::OP
                    | Class::QU
                    | Class::QUPI
                    | Class::QUPF
                    | Class::GL
                    | Class::SP
                    | Class::ZW
            )
        };
        self.quote_initial = if c == Class::SP {
            was.quote_initial
        } else {
            c == Class::QUPI && (sot || ctx(prev))
        };
        self.quote_after_narrow =
            matches!(c, Class::QU | Class::QUPI | Class::QUPF) && (sot || !was.prev_wide);
        let hyphen = matches!(c, Class::HY | Class::HH);
        self.hyphen_initial = hyphen
            && (sot
                || matches!(
                    prev,
                    Class::BK
                        | Class::CR
                        | Class::LF
                        | Class::NL
                        | Class::SP
                        | Class::ZW
                        | Class::CB
                        | Class::GL
                ));
        self.hyphen_after_hebrew = hyphen && prev == Class::HL;
        self.ri_odd = c == Class::RI && !(prev == Class::RI && was.ri_odd);
        self.number = match (c, was.number) {
            (Class::NU, _) => 1,
            (Class::SY | Class::IS, 1) => 1,
            (Class::CL | Class::CP, 1) => 2,
            _ => 0,
        };
        self.aksara = if matches!(c, Class::AK | Class::AS) || now.circle {
            1
        } else if c == Class::VI && was.aksara == 1 {
            2
        } else {
            0
        };
        if c == Class::SP {
            self.pre_space = if in_spaces { was.pre_space } else { prev };
            self.prev = Class::SP;
        } else {
            self.prev = c;
        }
        self.prev_wide = now.wide;
        self.prev_pictograph = now.pictograph;
        result
    }
}
