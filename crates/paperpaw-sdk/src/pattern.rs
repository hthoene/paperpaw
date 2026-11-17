//! Patterns of text: a small part of regular expressions, enough to find
//! words and phrases (to mark them on a page, see
//! `decorations::PatternDecorator`),
//! without a library and with bounded work, so a pattern from a data
//! plugin cannot hang the device.
//!
//! What a pattern can say:
//!
//! | Pattern | Matches |
//! |---|---|
//! | `abc` | the characters themselves |
//! | `.` | any character |
//! | `[abc]`, `[a-z]`, `[^0-9]` | one of (or none of) these characters |
//! | `\d`, `\w`, `\s` (`\D`, `\W`, `\S`) | a digit, a letter, digit or `_`, a space (and the others) |
//! | `\b` | the edge of a word |
//! | `^`, `$` | the start and the end of the text |
//! | `x*`, `x+`, `x?`, `x{2}`, `x{2,}`, `x{2,5}` | repeats, as many as fit |
//! | `a\|b` | either |
//! | `(...)`, `(?:...)` | a group |
//! | `(?i)` at the start | letters in any case |
//! | `\.`, `\\`, `\(`, `\n`, `\t` … | the character escaped |
//!
//! Letters, digits and spaces are those of Unicode. Patterns are at most
//! [`MAX_PATTERN`] characters, repeats at most [`MAX_REPEAT`], and a search
//! gives up after a bounded amount of work (finding nothing more).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// The longest pattern, in characters.
pub const MAX_PATTERN: usize = 256;

/// The most a repeat may count (`x{n}`, and `*` and `+` within groups).
pub const MAX_REPEAT: u32 = 100;

/// Steps a whole search may take before it gives up.
const BUDGET: u32 = 200_000;

/// How deep matching may go into groups and repeats of groups.
const MAX_DEPTH: u32 = 64;

/// Why a pattern cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternError {
    /// Longer than [`MAX_PATTERN`].
    TooLong,
    /// Not of the syntax above, at this character (counted from 0).
    Syntax(usize),
    /// A repeat larger than [`MAX_REPEAT`].
    TooManyRepeats,
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(f, "longer than {MAX_PATTERN} characters"),
            Self::Syntax(at) => write!(f, "not understood at character {}", at + 1),
            Self::TooManyRepeats => write!(f, "a repeat larger than {MAX_REPEAT}"),
        }
    }
}

/// A set of characters (`[a-z]`, `\d`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Class {
    negated: bool,
    items: Vec<Item>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

impl Item {
    fn has(self, c: char) -> bool {
        match self {
            Item::Range(lo, hi) => lo <= c && c <= hi,
            Item::Digit(yes) => c.is_numeric() == yes,
            Item::Word(yes) => is_word(c) == yes,
            Item::Space(yes) => c.is_whitespace() == yes,
        }
    }
}

impl Class {
    fn has(&self, c: char, fold: bool) -> bool {
        let found = self
            .items
            .iter()
            .any(|&item| item.has(c) || fold && (item.has(lower(c)) || item.has(upper(c))));
        found != self.negated
    }
}

/// A part of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Char(char),
    Any,
    Class(Class),
    Start,
    End,
    Edge,
    Group(Box<Alternatives>),
    Repeat(Box<Node>, u32, u32),
}

/// Alternatives, each a sequence of nodes.
type Alternatives = Vec<Vec<Node>>;

/// A pattern made from its text, to search texts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    alternatives: Alternatives,
    fold: bool,
    source: String,
}

impl Pattern {
    /// The pattern `source` (see the module's table).
    pub fn new(source: &str) -> Result<Pattern, PatternError> {
        if source.chars().count() > MAX_PATTERN {
            return Err(PatternError::TooLong);
        }
        let (fold, body) = match source.strip_prefix("(?i)") {
            Some(rest) => (true, rest),
            None => (false, source),
        };
        let offset = source.len() - body.len();
        let chars: Vec<(usize, char)> = body
            .char_indices()
            .map(|(i, c)| (source[..i + offset].chars().count(), c))
            .collect();
        let mut parser = Parser {
            chars: &chars,
            at: 0,
        };
        let alternatives = parser.alternatives(0)?;
        if parser.at < chars.len() {
            return Err(PatternError::Syntax(parser.position()));
        }
        Ok(Pattern {
            alternatives,
            fold,
            source: String::from(source),
        })
    }

    /// The text it was made from.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Whether it is found in `text`.
    pub fn is_match(&self, text: &str) -> bool {
        self.find_iter(text).next().is_some()
    }

    /// Where it is found in `text`, one place after another, not
    /// overlapping: byte ranges, at character boundaries. An empty match
    /// is not given; a search that takes too long ends early.
    pub fn find_iter<'p, 't>(&'p self, text: &'t str) -> Matches<'p, 't> {
        Matches {
            pattern: self,
            text,
            at: 0,
            budget: BUDGET,
        }
    }
}

/// The places a [`Pattern`] is found in a text (see
/// [`Pattern::find_iter`]).
#[derive(Debug)]
pub struct Matches<'p, 't> {
    pattern: &'p Pattern,
    text: &'t str,
    at: usize,
    budget: u32,
}

impl Iterator for Matches<'_, '_> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        while self.at <= self.text.len() {
            let start = self.at;
            let mut matcher = Matcher {
                text: self.text,
                fold: self.pattern.fold,
                budget: &mut self.budget,
            };
            let end = matcher.alternatives(&self.pattern.alternatives, start, 0);
            if self.budget == 0 {
                self.at = self.text.len() + 1;
                return None;
            }
            // On to the next character.
            let step = self.text[start..].chars().next().map_or(1, char::len_utf8);
            match end {
                Some(end) if end > start => {
                    self.at = end;
                    return Some((start, end));
                }
                _ => self.at = start + step,
            }
        }
        None
    }
}

struct Parser<'c> {
    /// The characters with their place in the whole pattern.
    chars: &'c [(usize, char)],
    at: usize,
}

impl Parser<'_> {
    fn position(&self) -> usize {
        self.chars
            .get(self.at)
            .map_or_else(|| self.chars.last().map_or(0, |c| c.0 + 1), |c| c.0)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).map(|c| c.1)
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += 1;
        Some(c)
    }

    fn error<T>(&self) -> Result<T, PatternError> {
        Err(PatternError::Syntax(self.position()))
    }

    fn alternatives(&mut self, depth: u32) -> Result<Alternatives, PatternError> {
        if depth > MAX_DEPTH {
            return self.error();
        }
        let mut alternatives = alloc::vec![self.sequence(depth)?];
        while self.peek() == Some('|') {
            self.at += 1;
            alternatives.push(self.sequence(depth)?);
        }
        Ok(alternatives)
    }

    fn sequence(&mut self, depth: u32) -> Result<Vec<Node>, PatternError> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom(depth)?;
            let node = self.repeat(atom)?;
            nodes.push(node);
        }
        Ok(nodes)
    }

    fn atom(&mut self, depth: u32) -> Result<Node, PatternError> {
        let Some(c) = self.next() else {
            return self.error();
        };
        Ok(match c {
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '[' => Node::Class(self.class()?),
            '(' => {
                if self.peek() == Some('?') {
                    self.at += 1;
                    if self.next() != Some(':') {
                        self.at -= 1;
                        return self.error();
                    }
                }
                let inner = self.alternatives(depth + 1)?;
                if self.next() != Some(')') {
                    return self.error();
                }
                Node::Group(Box::new(inner))
            }
            '\\' => match self.escape()? {
                Escaped::Char(c) => Node::Char(c),
                Escaped::Item(item) => Node::Class(Class {
                    negated: false,
                    items: alloc::vec![item],
                }),
                Escaped::Edge => Node::Edge,
            },
            '*' | '+' | '?' | '{' | ')' => {
                self.at -= 1;
                return self.error();
            }
            c => Node::Char(c),
        })
    }

    fn escape(&mut self) -> Result<Escaped, PatternError> {
        let Some(c) = self.next() else {
            return self.error();
        };
        Ok(match c {
            'd' => Escaped::Item(Item::Digit(true)),
            'D' => Escaped::Item(Item::Digit(false)),
            'w' => Escaped::Item(Item::Word(true)),
            'W' => Escaped::Item(Item::Word(false)),
            's' => Escaped::Item(Item::Space(true)),
            'S' => Escaped::Item(Item::Space(false)),
            'b' => Escaped::Edge,
            'n' => Escaped::Char('\n'),
            't' => Escaped::Char('\t'),
            c if c.is_alphanumeric() => {
                self.at -= 1;
                return self.error();
            }
            c => Escaped::Char(c),
        })
    }

    fn class(&mut self) -> Result<Class, PatternError> {
        let negated = self.peek() == Some('^');
        if negated {
            self.at += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.next() else {
                return self.error();
            };
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = match c {
                '\\' => match self.escape()? {
                    Escaped::Char(c) => c,
                    Escaped::Item(item) => {
                        items.push(item);
                        continue;
                    }
                    Escaped::Edge => return self.error(),
                },
                c => c,
            };
            // A range, unless the `-` ends the class.
            let range =
                self.peek() == Some('-') && self.chars.get(self.at + 1).is_some_and(|c| c.1 != ']');
            if !range {
                items.push(Item::Range(lo, lo));
                continue;
            }
            self.at += 1;
            let hi = match self.next() {
                Some('\\') => match self.escape()? {
                    Escaped::Char(c) => c,
                    _ => return self.error(),
                },
                Some(c) => c,
                None => return self.error(),
            };
            if hi < lo {
                return self.error();
            }
            items.push(Item::Range(lo, hi));
        }
        Ok(Class { negated, items })
    }

    fn repeat(&mut self, atom: Node) -> Result<Node, PatternError> {
        let (min, max) = match self.peek() {
            Some('*') => (0, u32::MAX),
            Some('+') => (1, u32::MAX),
            Some('?') => (0, 1),
            Some('{') => {
                self.at += 1;
                let min = self.number()?;
                let max = match self.next() {
                    Some('}') => {
                        self.at -= 1;
                        min
                    }
                    Some(',') if self.peek() == Some('}') => u32::MAX,
                    Some(',') => self.number()?,
                    _ => return self.error(),
                };
                if self.peek() != Some('}') || max < min {
                    return self.error();
                }
                if min > MAX_REPEAT || max != u32::MAX && max > MAX_REPEAT {
                    return Err(PatternError::TooManyRepeats);
                }
                (min, max)
            }
            _ => return Ok(atom),
        };
        self.at += 1;
        if matches!(atom, Node::Start | Node::End | Node::Edge) {
            self.at -= 1;
            return self.error();
        }
        Ok(Node::Repeat(Box::new(atom), min, max))
    }

    fn number(&mut self) -> Result<u32, PatternError> {
        let mut n: u32 = 0;
        let mut digits = 0;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
            n = n.saturating_mul(10).saturating_add(d);
            digits += 1;
            self.at += 1;
        }
        if digits == 0 {
            return self.error();
        }
        Ok(n)
    }
}

enum Escaped {
    Char(char),
    Item(Item),
    Edge,
}

/// Matching at one place: a backtracking search, each step counted
/// against the search's budget.
struct Matcher<'t, 'b> {
    text: &'t str,
    fold: bool,
    budget: &'b mut u32,
}

impl Matcher<'_, '_> {
    /// Where the first alternative that matches at `at` ends.
    fn alternatives(
        &mut self,
        alternatives: &Alternatives,
        at: usize,
        depth: u32,
    ) -> Option<usize> {
        alternatives
            .iter()
            .find_map(|sequence| self.sequence(sequence, at, depth, &mut |_, end| Some(end)))
    }

    /// Matches `nodes` at `at`, then `rest` from where they end.
    fn sequence(
        &mut self,
        nodes: &[Node],
        at: usize,
        depth: u32,
        rest: &mut dyn FnMut(&mut Self, usize) -> Option<usize>,
    ) -> Option<usize> {
        if depth > MAX_DEPTH {
            return None;
        }
        // Single characters, anchors and edges one after another, without
        // going deeper (a long pattern of letters stays shallow).
        let (mut at, mut nodes) = (at, nodes);
        let (node, after) = loop {
            if *self.budget == 0 {
                return None;
            }
            *self.budget -= 1;
            let Some((node, after)) = nodes.split_first() else {
                return rest(self, at);
            };
            if matches!(node, Node::Repeat(..) | Node::Group(_)) {
                break (node, after);
            }
            at = self.one(node, at)?;
            nodes = after;
        };
        let depth = depth + 1;
        match node {
            Node::Repeat(inner, min, max)
                if matches!(**inner, Node::Char(_) | Node::Any | Node::Class(_)) =>
            {
                // One character at a time: as many as fit, then a
                // character fewer each try.
                let (mut end, mut count) = (at, 0);
                while count < *max {
                    let Some(next) = self.one(inner, end) else {
                        break;
                    };
                    (end, count) = (next, count + 1);
                    if *self.budget == 0 {
                        return None;
                    }
                    *self.budget -= 1;
                }
                loop {
                    if count < *min {
                        return None;
                    }
                    if let Some(found) = self.sequence(after, end, depth, rest) {
                        return Some(found);
                    }
                    let back = self.text[..end].chars().next_back()?;
                    if count == 0 {
                        return None;
                    }
                    (end, count) = (end - back.len_utf8(), count - 1);
                }
            }
            Node::Repeat(inner, min, max) => {
                self.repeat(inner, *min, *max, 0, at, after, depth, rest)
            }
            Node::Group(alternatives) => {
                for sequence in alternatives.iter() {
                    let found = self.sequence(sequence, at, depth + 1, &mut |m, end| {
                        m.sequence(after, end, depth + 1, rest)
                    });
                    if found.is_some() {
                        return found;
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// A repeat of a group: `count` times so far, as many more as fit.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    fn repeat(
        &mut self,
        inner: &Node,
        min: u32,
        max: u32,
        count: u32,
        at: usize,
        after: &[Node],
        depth: u32,
        rest: &mut dyn FnMut(&mut Self, usize) -> Option<usize>,
    ) -> Option<usize> {
        if count < max.min(MAX_REPEAT) {
            let more = self.sequence(
                core::slice::from_ref(inner),
                at,
                depth + 1,
                &mut |m, end| {
                    // A repeat that takes nothing goes no further.
                    if end == at {
                        return None;
                    }
                    m.repeat(inner, min, max, count + 1, end, after, depth + 1, rest)
                },
            );
            if more.is_some() {
                return more;
            }
        }
        if count >= min {
            self.sequence(after, at, depth, rest)
        } else {
            None
        }
    }

    /// Where `node` (one character, an anchor or an edge) ends at `at`.
    fn one(&self, node: &Node, at: usize) -> Option<usize> {
        let text = self.text;
        let next = text[at..].chars().next();
        let after = |c: char| at + c.len_utf8();
        match node {
            Node::Char(want) => {
                let c = next?;
                let same = c == *want || self.fold && lower(c) == lower(*want);
                same.then(|| after(c))
            }
            Node::Any => next.map(after),
            Node::Class(class) => {
                let c = next?;
                class.has(c, self.fold).then(|| after(c))
            }
            Node::Start => (at == 0).then_some(at),
            Node::End => (at == text.len()).then_some(at),
            Node::Edge => {
                let before = text[..at].chars().next_back().is_some_and(is_word);
                let here = next.is_some_and(is_word);
                (before != here).then_some(at)
            }
            Node::Group(_) | Node::Repeat(..) => None,
        }
    }
}

/// A letter, digit or `_`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// The texts `pattern` finds in `text`.
    fn found<'t>(pattern: &str, text: &'t str) -> Vec<&'t str> {
        let pattern = Pattern::new(pattern).unwrap();
        pattern.find_iter(text).map(|(a, b)| &text[a..b]).collect()
    }

    #[test]
    fn characters_classes_and_repeats() {
        assert_eq!(found("cat", "a cat, a catalogue"), ["cat", "cat"]);
        assert_eq!(found("c.t", "cat cot cut ct"), ["cat", "cot", "cut"]);
        assert_eq!(found("[0-9]+", "page 12 of 340"), ["12", "340"]);
        assert_eq!(found(r"\d{2,3}", "1 22 333 4444"), ["22", "333", "444"]);
        assert_eq!(
            found("colou?r", "color colour colouur"),
            ["color", "colour"]
        );
        assert_eq!(found("[^ ]+s", "cats and dogs"), ["cats", "dogs"]);
        assert_eq!(found("a*", "baaa"), ["aaa"]);
        assert_eq!(
            found(r"\w+-\w+", "well-known and so-called"),
            ["well-known", "so-called"]
        );
    }

    #[test]
    fn words_alternatives_groups_and_case() {
        assert_eq!(found(r"\bcat\b", "cat concat cats cat."), ["cat", "cat"]);
        assert_eq!(
            found("(?i)alice", "Alice ALICE alice"),
            ["Alice", "ALICE", "alice"]
        );
        assert_eq!(found("(?i)[a-c]+", "ABC def"), ["ABC"]);
        assert_eq!(found("cat|dog", "dog and cat"), ["dog", "cat"]);
        assert_eq!(found(r"(ab)+c", "ababc abc ac"), ["ababc", "abc"]);
        assert_eq!(
            found(r"(?:Mr|Mrs)\. \w+", "Mr. Smith and Mrs. Jones"),
            ["Mr. Smith", "Mrs. Jones"]
        );
        assert_eq!(found("^The", "The end. The"), ["The"]);
        assert_eq!(found("end$", "end of the end"), ["end"]);
        // Letters beyond ASCII.
        assert_eq!(found(r"\w+", "Straße größer"), ["Straße", "größer"]);
        assert_eq!(found("(?i)ÄRGER", "ärger"), ["ärger"]);
    }

    #[test]
    fn escapes_and_odd_classes() {
        assert_eq!(found(r"\.\.\.", "wait... what"), ["..."]);
        assert_eq!(found(r"[\d.]+", "pi is 3.14."), ["3.14."]);
        assert_eq!(found("[a-]", "-a"), ["-", "a"]);
        assert_eq!(found("[]a]", "]a"), ["]", "a"]);
        assert_eq!(found(r"\(\w+\)", "a (note) here"), ["(note)"]);
    }

    #[test]
    fn mistakes_are_refused() {
        for bad in [
            "(", ")", "[a", "a{2", "*a", "a**", r"\q", "[z-a]", "(?x)", r"\b+",
        ] {
            assert!(
                matches!(Pattern::new(bad), Err(PatternError::Syntax(_))),
                "{bad}"
            );
        }
        assert_eq!(Pattern::new("a{101}"), Err(PatternError::TooManyRepeats));
        assert_eq!(Pattern::new(&"a".repeat(300)), Err(PatternError::TooLong));
        assert_eq!(Pattern::new("ab(c").unwrap_err(), PatternError::Syntax(4));
    }

    #[test]
    fn long_patterns_and_long_texts_stay_shallow() {
        let long = "ab".repeat(120);
        let text = format!("x{long}y");
        assert_eq!(found(&long, &text), [long.as_str()]);
        assert_eq!(found("a+", &"a".repeat(50_000)).len(), 1);
    }

    #[test]
    fn hostile_patterns_end_soon() {
        // Nested repeats that try every way to split the text.
        let text = "a".repeat(2_000) + "b";
        for hostile in ["(a+)+c", "(a|aa)+c", "(a*)*c", "((a+)+)+c"] {
            let pattern = Pattern::new(hostile).unwrap();
            assert_eq!(pattern.find_iter(&text).count(), 0, "{hostile}");
        }
        // Deep groups are refused or end; nothing overflows the stack.
        let deep = "(".repeat(200) + &")".repeat(200);
        assert!(Pattern::new(&deep).is_err());
    }

    #[test]
    fn empty_matches_are_not_given() {
        assert!(found("x*", "abc").is_empty());
        assert!(found(r"\b", "a b").is_empty());
        assert!(!Pattern::new("x?").unwrap().is_match("abc"));
    }
}
