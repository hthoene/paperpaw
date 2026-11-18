//! Crosswords with no host: the Across Lite `.puz` file and the `ipuz` text
//! (JSON), read into one small grid with its numbering and clues, plus the
//! player's letters, their check and the kept text of a game.

use alloc::{string::String, vec::Vec};

/// The largest grid side read.
pub const SIDE_MAX: usize = 25;
/// The most bytes of a file read.
pub const FILE_MAX: usize = 16 * 1024;

/// An empty square of the player's letters; a black square is `.` in both.
pub const EMPTY: u8 = b'-';
pub const BLOCK: u8 = b'.';

/// A clue: its number, where its word starts, how long it is, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clue {
    pub number: u16,
    pub cell: u16,
    pub len: u8,
    pub text: String,
}

/// A crossword.
#[derive(Debug, Clone)]
pub struct Crossword {
    pub width: u8,
    pub height: u8,
    pub title: String,
    /// The answers (letters, or `.` for a black square), row by row.
    pub solution: Vec<u8>,
    /// The player's letters, `-` for none.
    pub letters: Vec<u8>,
    /// The clue number at each square, 0 for none.
    pub numbers: Vec<u16>,
    pub across: Vec<Clue>,
    pub down: Vec<Clue>,
}

/// Why a file was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Not a crossword file, or damaged.
    Format,
    /// A scrambled `.puz` (its solution is locked).
    Scrambled,
    /// Larger than this reader takes.
    Large,
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(*b)).collect()
}

impl Crossword {
    fn cell_count(&self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }

    fn is_block(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 || x >= isize::from(self.width) || y >= isize::from(self.height) {
            return true;
        }
        let at = usize::try_from(y).unwrap_or(0) * usize::from(self.width)
            + usize::try_from(x).unwrap_or(0);
        self.solution.get(at).is_none_or(|c| *c == BLOCK)
    }

    /// Numbers the squares and finds the words: a square that starts an
    /// across or a down word of two letters or more is numbered.
    fn number(&mut self) -> Vec<(u16, bool, bool)> {
        let mut starts = Vec::new();
        self.numbers = alloc::vec![0; self.cell_count()];
        let mut n = 0u16;
        for y in 0..isize::from(self.height) {
            for x in 0..isize::from(self.width) {
                if self.is_block(x, y) {
                    continue;
                }
                let across = self.is_block(x - 1, y) && !self.is_block(x + 1, y);
                let down = self.is_block(x, y - 1) && !self.is_block(x, y + 1);
                if across || down {
                    n += 1;
                    let at = usize::try_from(y).unwrap_or(0) * usize::from(self.width)
                        + usize::try_from(x).unwrap_or(0);
                    self.numbers[at] = n;
                    starts.push((n, across, down));
                }
            }
        }
        starts
    }

    fn word_len(&self, at: usize, across: bool) -> u8 {
        let w = usize::from(self.width);
        let (mut x, mut y) = (at % w, at / w);
        let mut len = 0u8;
        while !self.is_block(
            isize::try_from(x).unwrap_or(0),
            isize::try_from(y).unwrap_or(0),
        ) {
            len = len.saturating_add(1);
            if across {
                x += 1;
            } else {
                y += 1;
            }
        }
        len
    }

    fn clue(&self, number: u16, across: bool, text: String) -> Option<Clue> {
        let at = self.numbers.iter().position(|n| *n == number)?;
        Some(Clue {
            number,
            cell: u16::try_from(at).ok()?,
            len: self.word_len(at, across),
            text,
        })
    }

    fn new(width: usize, height: usize, solution: Vec<u8>) -> Result<Self, Fault> {
        if width == 0 || height == 0 || width > SIDE_MAX || height > SIDE_MAX {
            return Err(Fault::Format);
        }
        let letters = solution
            .iter()
            .map(|c| if *c == BLOCK { BLOCK } else { EMPTY })
            .collect();
        Ok(Self {
            width: u8::try_from(width).map_err(|_| Fault::Format)?,
            height: u8::try_from(height).map_err(|_| Fault::Format)?,
            title: String::new(),
            solution,
            letters,
            numbers: Vec::new(),
            across: Vec::new(),
            down: Vec::new(),
        })
    }

    /// Reads an Across Lite `.puz` file.
    ///
    /// # Errors
    /// [`Fault`] for what is not one, a scrambled one, or a grid too large.
    pub fn from_puz(data: &[u8]) -> Result<Self, Fault> {
        if data.get(2..0x0E) != Some(b"ACROSS&DOWN\0") {
            return Err(Fault::Format);
        }
        let (w, h) = (
            usize::from(*data.get(0x2C).ok_or(Fault::Format)?),
            usize::from(*data.get(0x2D).ok_or(Fault::Format)?),
        );
        let count = usize::from(u16::from_le_bytes([
            *data.get(0x2E).ok_or(Fault::Format)?,
            *data.get(0x2F).ok_or(Fault::Format)?,
        ]));
        if w > SIDE_MAX || h > SIDE_MAX {
            return Err(Fault::Large);
        }
        let scrambled = u16::from_le_bytes([
            *data.get(0x32).ok_or(Fault::Format)?,
            *data.get(0x33).ok_or(Fault::Format)?,
        ]);
        if scrambled != 0 {
            return Err(Fault::Scrambled);
        }
        let cells = w * h;
        let solution: Vec<u8> = data
            .get(0x34..0x34 + cells)
            .ok_or(Fault::Format)?
            .iter()
            .map(|c| {
                if *c == b'.' {
                    BLOCK
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        let mut cross = Self::new(w, h, solution)?;
        // The player's letters the file kept, where there are some.
        if let Some(state) = data.get(0x34 + cells..0x34 + 2 * cells) {
            for (letter, kept) in cross.letters.iter_mut().zip(state) {
                if *letter != BLOCK && kept.is_ascii_alphabetic() {
                    *letter = kept.to_ascii_uppercase();
                }
            }
        }
        let mut strings = data
            .get(0x34 + 2 * cells..)
            .ok_or(Fault::Format)?
            .split(|b| *b == 0);
        cross.title = latin1(strings.next().ok_or(Fault::Format)?);
        strings.next().ok_or(Fault::Format)?;
        strings.next().ok_or(Fault::Format)?;
        let starts = cross.number();
        let mut clues = strings.take(count);
        for (number, across, down) in starts {
            for is_across in [true, false] {
                if (is_across && !across) || (!is_across && !down) {
                    continue;
                }
                let text = latin1(clues.next().ok_or(Fault::Format)?);
                let clue = cross.clue(number, is_across, text).ok_or(Fault::Format)?;
                if is_across {
                    cross.across.push(clue);
                } else {
                    cross.down.push(clue);
                }
            }
        }
        Ok(cross)
    }

    /// Reads `ipuz` text: its `puzzle` and `solution` grids and `clues`.
    ///
    /// # Errors
    /// [`Fault`] for what is not a crossword in it.
    pub fn from_ipuz(text: &str) -> Result<Self, Fault> {
        let root = Json::new(text);
        let solution_grid = root.member("solution").ok_or(Fault::Format)?;
        let mut rows = Vec::new();
        let mut width = 0;
        for row in solution_grid.items() {
            let mut line = Vec::new();
            for cell in row.items() {
                line.push(solution_letter(&cell));
            }
            width = width.max(line.len());
            rows.push(line);
        }
        let height = rows.len();
        if width > SIDE_MAX || height > SIDE_MAX {
            return Err(Fault::Large);
        }
        let mut solution = Vec::with_capacity(width * height);
        for line in &rows {
            for x in 0..width {
                solution.push(line.get(x).copied().unwrap_or(BLOCK));
            }
        }
        let mut cross = Self::new(width, height, solution)?;
        if let Some(title) = root.member("title").and_then(|t| t.string()) {
            cross.title = title;
        }
        cross.number();
        let clues = root.member("clues").ok_or(Fault::Format)?;
        for (name, across) in [("Across", true), ("Down", false)] {
            let Some(list) = clues.member(name) else {
                continue;
            };
            for item in list.items() {
                let (number, text) = clue_of(&item).ok_or(Fault::Format)?;
                let clue = cross.clue(number, across, text).ok_or(Fault::Format)?;
                if across {
                    cross.across.push(clue);
                } else {
                    cross.down.push(clue);
                }
            }
        }
        if cross.across.is_empty() && cross.down.is_empty() {
            return Err(Fault::Format);
        }
        Ok(cross)
    }

    /// Reads a file's bytes as `.puz` or, failing that, as `ipuz`.
    ///
    /// # Errors
    /// [`Fault`]: too large, or neither.
    pub fn from_bytes(data: &[u8]) -> Result<Self, Fault> {
        if data.len() > FILE_MAX {
            return Err(Fault::Large);
        }
        match Self::from_puz(data) {
            Err(Fault::Format) => core::str::from_utf8(data)
                .map_err(|_| Fault::Format)
                .and_then(Self::from_ipuz),
            other => other,
        }
    }

    #[must_use]
    pub fn is_block_at(&self, at: usize) -> bool {
        self.solution.get(at).is_none_or(|c| *c == BLOCK)
    }

    /// The clue of the word through square `at` in a direction.
    #[must_use]
    pub fn word_at(&self, at: usize, across: bool) -> Option<&Clue> {
        let w = usize::from(self.width);
        let list = if across { &self.across } else { &self.down };
        list.iter().find(|c| {
            let start = usize::from(c.cell);
            let len = usize::from(c.len);
            if across {
                at / w == start / w && at >= start && at < start + len
            } else {
                at % w == start % w
                    && at >= start
                    && at < start + len * w
                    && (at - start).is_multiple_of(w)
            }
        })
    }

    /// The squares of a clue's word.
    pub fn cells_of(&self, clue: &Clue, across: bool) -> impl Iterator<Item = usize> + use<> {
        let start = usize::from(clue.cell);
        let step = if across { 1 } else { usize::from(self.width) };
        (0..usize::from(clue.len)).map(move |i| start + i * step)
    }

    #[must_use]
    pub fn solved(&self) -> bool {
        self.letters == self.solution
    }

    #[must_use]
    pub fn wrong(&self, at: usize) -> bool {
        matches!((self.letters.get(at), self.solution.get(at)), (Some(l), Some(s)) if *l != EMPTY && l != s)
    }

    #[must_use]
    pub fn wrong_count(&self) -> usize {
        (0..self.cell_count()).filter(|a| self.wrong(*a)).count()
    }

    #[must_use]
    pub fn blank_count(&self) -> usize {
        self.letters.iter().filter(|l| **l == EMPTY).count()
    }

    /// Puts the letters of `text` into the word of `clue`, from its start;
    /// what is not a letter (a space, `-`) leaves the square as it was.
    pub fn fill(&mut self, clue: &Clue, across: bool, text: &str) {
        let squares: Vec<usize> = self.cells_of(clue, across).collect();
        for (at, c) in squares.into_iter().zip(text.chars()) {
            if let Some(slot) = self.letters.get_mut(at) {
                if c.is_ascii_alphabetic() {
                    *slot = c.to_ascii_uppercase() as u8;
                } else if c == '_' || c == '-' {
                    *slot = EMPTY;
                }
            }
        }
    }

    /// The letters of a word as text, `-` for an empty square.
    #[must_use]
    pub fn word_text(&self, clue: &Clue, across: bool) -> String {
        self.cells_of(clue, across)
            .map(|a| char::from(self.letters.get(a).copied().unwrap_or(EMPTY)))
            .collect()
    }

    /// Fills the word of `clue` with its answer.
    pub fn reveal(&mut self, clue: &Clue, across: bool) {
        let squares: Vec<usize> = self.cells_of(clue, across).collect();
        for at in squares {
            if let (Some(slot), Some(s)) = (self.letters.get_mut(at), self.solution.get(at)) {
                *slot = *s;
            }
        }
    }

    /// The kept text of a game: the letters.
    #[must_use]
    pub fn text(&self) -> String {
        latin1(&self.letters)
    }

    /// Takes the player's letters from a kept text of the same grid.
    pub fn restore(&mut self, kept: &str) -> bool {
        let kept = kept.trim().as_bytes();
        if kept.len() != self.letters.len() {
            return false;
        }
        for ((slot, k), s) in self.letters.iter_mut().zip(kept).zip(&self.solution) {
            if *s == BLOCK {
                continue;
            }
            *slot = if k.is_ascii_alphabetic() {
                k.to_ascii_uppercase()
            } else {
                EMPTY
            };
        }
        true
    }
}

fn solution_letter(cell: &Json<'_>) -> u8 {
    let value = match cell.kind() {
        Kind::Object => cell.member("value"),
        _ => Some(cell.clone()),
    };
    value
        .and_then(|v| v.string())
        .and_then(|s| s.chars().next())
        .filter(char::is_ascii_alphanumeric)
        .map_or(BLOCK, |c| c.to_ascii_uppercase() as u8)
}

fn clue_of(item: &Json<'_>) -> Option<(u16, String)> {
    let (number, text) = match item.kind() {
        Kind::Array => {
            let mut parts = item.items();
            (parts.next()?, parts.next()?)
        }
        Kind::Object => (item.member("number")?, item.member("clue")?),
        _ => return None,
    };
    Some((number.number()?, text.string()?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Array,
    Object,
    Text,
    Number,
    Other,
}

/// A value of a JSON text, found by scanning it where it is: nothing is
/// built but the strings and numbers asked for.
#[derive(Debug, Clone)]
struct Json<'a> {
    text: &'a str,
}

fn skip_space(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// The end of the value that starts at `i`.
fn value_end(b: &[u8], i: usize) -> Option<usize> {
    let i = skip_space(b, i);
    match *b.get(i)? {
        b'"' => string_end(b, i),
        open @ (b'[' | b'{') => {
            let close = if open == b'[' { b']' } else { b'}' };
            let mut at = skip_space(b, i + 1);
            if b.get(at) == Some(&close) {
                return Some(at + 1);
            }
            loop {
                if open == b'{' {
                    at = skip_space(b, string_end(b, skip_space(b, at))?);
                    if b.get(at) != Some(&b':') {
                        return None;
                    }
                    at += 1;
                }
                at = skip_space(b, value_end(b, at)?);
                match b.get(at)? {
                    b',' => at += 1,
                    c if *c == close => return Some(at + 1),
                    _ => return None,
                }
            }
        }
        _ => {
            let mut at = i;
            while b
                .get(at)
                .is_some_and(|c| !matches!(c, b',' | b']' | b'}') && !c.is_ascii_whitespace())
            {
                at += 1;
            }
            (at > i).then_some(at)
        }
    }
}

fn string_end(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let mut at = i + 1;
    loop {
        match *b.get(at)? {
            b'"' => return Some(at + 1),
            b'\\' => at += 2,
            _ => at += 1,
        }
    }
}

impl<'a> Json<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text: text.trim_start_matches('\u{feff}').trim_start(),
        }
    }

    fn kind(&self) -> Kind {
        match self.text.as_bytes().first() {
            Some(b'[') => Kind::Array,
            Some(b'{') => Kind::Object,
            Some(b'"') => Kind::Text,
            Some(b'0'..=b'9' | b'-') => Kind::Number,
            _ => Kind::Other,
        }
    }

    /// The value that starts at `i` and where it ends.
    fn value_at(&self, i: usize) -> Option<(Json<'a>, usize)> {
        let b = self.text.as_bytes();
        let i = skip_space(b, i);
        let end = value_end(b, i)?;
        Some((
            Json {
                text: self.text.get(i..end)?,
            },
            end,
        ))
    }

    /// The member `name` of an object.
    fn member(&self, name: &str) -> Option<Json<'a>> {
        let b = self.text.as_bytes();
        if b.first() != Some(&b'{') {
            return None;
        }
        let mut at = skip_space(b, 1);
        while b.get(at) == Some(&b'"') {
            let key_end = string_end(b, at)?;
            let key = Json {
                text: self.text.get(at..key_end)?,
            }
            .string()?;
            at = skip_space(b, key_end);
            if b.get(at) != Some(&b':') {
                return None;
            }
            let (value, end) = self.value_at(at + 1)?;
            if key == name {
                return Some(value);
            }
            let after = skip_space(b, end);
            if b.get(after) != Some(&b',') {
                return None;
            }
            at = skip_space(b, after + 1);
        }
        None
    }

    /// The items of an array.
    fn items(&self) -> impl Iterator<Item = Json<'a>> + use<'a> {
        let this = self.clone();
        let b = this.text.as_bytes();
        let mut at = (b.first() == Some(&b'[')).then(|| skip_space(b, 1));
        core::iter::from_fn(move || {
            let b = this.text.as_bytes();
            let i = at?;
            if b.get(i).is_none_or(|c| *c == b']') {
                at = None;
                return None;
            }
            let (value, end) = this.value_at(i)?;
            let after = skip_space(b, end);
            at = (b.get(after) == Some(&b',')).then(|| skip_space(b, after + 1));
            Some(value)
        })
    }

    /// A string value, unescaped; a number's own digits are text too.
    fn string(&self) -> Option<String> {
        if self.kind() != Kind::Text {
            return (self.kind() == Kind::Number).then(|| String::from(self.text));
        }
        let inner = self.text.get(1..self.text.len().checked_sub(1)?)?;
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next()? {
                'n' | 't' | 'r' => out.push(' '),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            }
        }
        Some(out)
    }

    fn number(&self) -> Option<u16> {
        self.string()?.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3x3 crossword with the middle square black:
    ///   A B C
    ///   D # E
    ///   F G H
    fn puz() -> Vec<u8> {
        let mut data = alloc::vec![0u8; 0x34];
        data[2..0x0E].copy_from_slice(b"ACROSS&DOWN\0");
        data[0x2C] = 3;
        data[0x2D] = 3;
        // Across: 1 (ABC), 3 (FGH); down: 1 (ADF), 2 (CEH); the clues come by number.
        data[0x2E] = 4;
        data.extend_from_slice(b"ABCD.EFGH");
        data.extend_from_slice(b"---------");
        for s in [
            "Tiny\0",
            "Me\0",
            "Free\0",
            "First\0",
            "Down one\0",
            "Down two\0",
            "Bottom\0",
            "A note\0",
        ] {
            data.extend_from_slice(s.as_bytes());
        }
        data
    }

    #[test]
    fn a_puz_file_is_read_with_numbers_and_clues() {
        let c = Crossword::from_puz(&puz()).unwrap();
        assert_eq!((c.width, c.height, c.title.as_str()), (3, 3, "Tiny"));
        assert_eq!(c.numbers, [1, 0, 2, 0, 0, 0, 3, 0, 0]);
        let across: Vec<_> = c
            .across
            .iter()
            .map(|k| (k.number, k.len, k.text.as_str()))
            .collect();
        assert_eq!(across, [(1, 3, "First"), (3, 3, "Bottom")]);
        let down: Vec<_> = c
            .down
            .iter()
            .map(|k| (k.number, k.len, k.text.as_str()))
            .collect();
        assert_eq!(down, [(1, 3, "Down one"), (2, 3, "Down two")]);
        assert_eq!(c.word_at(1, true).unwrap().number, 1);
        assert_eq!(c.word_at(5, false).unwrap().number, 2);
        assert!(c.word_at(4, true).is_none());
    }

    #[test]
    fn a_damaged_scrambled_or_large_file_is_refused() {
        assert_eq!(Crossword::from_bytes(b"hello").unwrap_err(), Fault::Format);
        let mut cut = puz();
        cut.truncate(0x34 + 5);
        assert_eq!(Crossword::from_puz(&cut).unwrap_err(), Fault::Format);
        let mut scrambled = puz();
        scrambled[0x32] = 1;
        assert_eq!(
            Crossword::from_puz(&scrambled).unwrap_err(),
            Fault::Scrambled
        );
        let mut wide = puz();
        wide[0x2C] = 40;
        assert_eq!(Crossword::from_puz(&wide).unwrap_err(), Fault::Large);
        assert_eq!(
            Crossword::from_bytes(&alloc::vec![0; FILE_MAX + 1]).unwrap_err(),
            Fault::Large
        );
        // Fewer clues than words.
        let mut few = puz();
        few[0x2E] = 1;
        assert!(Crossword::from_puz(&few).is_err());
    }

    #[test]
    fn typing_check_and_reveal() {
        let mut c = Crossword::from_puz(&puz()).unwrap();
        let one = c.across[0].clone();
        c.fill(&one, true, "abx");
        assert_eq!(c.word_text(&one, true), "ABX");
        assert!(c.wrong(2));
        assert_eq!(c.wrong_count(), 1);
        c.fill(&one, true, "__C");
        assert_eq!(c.word_text(&one, true), "--C");
        c.reveal(&one, true);
        assert_eq!(c.word_text(&one, true), "ABC");
        assert!(!c.solved());
        let (two, down) = (c.across[1].clone(), c.down.clone());
        c.fill(&two, true, "FGH");
        c.fill(&down[0], false, "ADF");
        c.fill(&down[1], false, "CEH");
        assert!(c.solved(), "{:?}", c.text());
        let kept = c.text();
        let mut again = Crossword::from_puz(&puz()).unwrap();
        assert!(again.restore(&kept));
        assert!(again.solved());
        assert!(!again.restore("short"));
    }

    #[test]
    fn an_ipuz_text_is_read() {
        let text = r##"{"version":"http://ipuz.org/v2","kind":["http://ipuz.org/crossword#1"],
          "title":"Tiny A",
          "dimensions":{"width":3,"height":3},
          "puzzle":[[1,0,2],[0,"#",0],[3,0,0]],
          "solution":[["A","B","C"],["D","#","E"],["F","G","H"]],
          "clues":{"Across":[[1,"First"],{"number":3,"clue":"Bottom"}],
                   "Down":[[1,"Down one"],["2","Down two"]]}}"##;
        let c = Crossword::from_bytes(text.as_bytes()).unwrap();
        assert_eq!(c.title, "Tiny A");
        assert_eq!(c.solution, b"ABCD.EFGH");
        assert_eq!(c.numbers, [1, 0, 2, 0, 0, 0, 3, 0, 0]);
        assert_eq!((c.across.len(), c.down.len()), (2, 2));
        assert_eq!(c.down[1].text, "Down two");
        assert!(Crossword::from_ipuz(r#"{"solution":[["A"]]}"#).is_err());
        assert!(Crossword::from_ipuz("[1,2").is_err());
    }
}
