//! Where a line may break inside Thai text, which has no spaces between
//! its words (#313): between two words of the table of words a Thai pack
//! carries (slot 6, see `bitmap.rs` and `tools/thai-dictionary.py`). Lao
//! and Khmer, also written without spaces, break the same way between the
//! words of their packs' tables (#314, `tools/word-table.py`), Lao's
//! letters and signs where Thai's are, Khmer's clusters a consonant with
//! those joined below it by coeng (U+17D2), its vowels and signs; and
//! Burmese (#397), its clusters a consonant with its signs, the consonant
//! asat or virama silence ending the syllable before, the one stacked
//! after virama held with it.
//!
//! A run of Thai letters is read from a word's start: of the words that
//! start there, the longest one after which the run ends or another word
//! starts, else the longest; where none starts, one cluster (a letter with
//! its vowels and tone marks, a vowel written before its letter with
//! it, a final letter after MAI HAN-AKAT or silenced by THANTHAKHAT with
//! it), and the next word is looked for after it. So the place after a
//! word depends only on the text from its start, as a line's lines do
//! (see `layout`). A line never breaks there inside a cluster, nor before
//! U+0E46 MAIYAMOK or U+0E2F PAIYANNOI (a word wider than the whole line
//! is still cut where it overflows, as any). Without a table (no Thai
//! pack open), Thai breaks only where it did before: at spaces and U+200B.

/// The most letters of a word in the table.
pub(crate) const LONGEST: usize = 15;
/// Words in a block of the table, the first of which is whole.
const BLOCK: usize = 16;
/// Letters read ahead from a word's start: the longest word, a sign after
/// it and the longest word after that.
const WINDOW: usize = 2 * LONGEST + 2;
/// The table's head, its counts and its version.
const HEAD: usize = 12;
const VERSION: u8 = 1;

/// The scripts written without spaces whose words a pack may carry, as a
/// table's head numbers them (0 Thai, 1 Lao, 2 Khmer, 3 Myanmar).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Script {
    Thai,
    Lao,
    Khmer,
    Myanmar,
}

impl Script {
    /// The scripts, in the order a table's head numbers them.
    pub(crate) const ALL: [Script; 4] = [Script::Thai, Script::Lao, Script::Khmer, Script::Myanmar];

    /// The first code point of its block: a letter in a table is its code
    /// point less it.
    const fn base(self) -> u32 {
        match self {
            Script::Thai => 0xE00,
            Script::Lao => 0xE80,
            Script::Khmer => 0x1780,
            Script::Myanmar => 0x1000,
        }
    }

    /// `c` as a table keeps it, if it is of the script's block.
    fn letter(self, c: char) -> Option<u8> {
        let l = u32::from(c).checked_sub(self.base())?;
        u8::try_from(l).ok().filter(|&l| l < 0x80)
    }

    /// The letter's properties (the flags below; 0 for none): looked up in
    /// a table, since a `match` per script and property made jump tables.
    fn flags(self, l: u8) -> u8 {
        FLAGS[self as usize]
            .get(usize::from(l))
            .copied()
            .unwrap_or(0)
    }

    /// Whether the letter is one words are made of (not a digit or
    /// punctuation; for Thai not U+0E3F BAHT, U+0E4F FONGMAN, U+0E5A,
    /// U+0E5B).
    fn in_word(self, l: u8) -> bool {
        self.flags(l) & IN_WORD != 0
    }

    /// Whether the letter stays with the one before it: vowels above, below
    /// and after, tone marks and signs (Thai's U+0E45, MAIYAMOK and
    /// PAIYANNOI, Lao's KO LA and ellipsis, Khmer's coeng and repetition
    /// sign).
    fn follows(self, l: u8) -> bool {
        self.flags(l) & FOLLOWS != 0
    }

    /// Whether the letter is a vowel written before the one it follows.
    fn leads(self, l: u8) -> bool {
        self.flags(l) & LEADS != 0
    }

    /// Whether the letter after this one stays with it: after Thai's MAI
    /// HAN-AKAT and Lao's MAI KAN their final letter, after coeng (Burmese
    /// virama) the consonant drawn below.
    fn holds_next(self, l: u8) -> bool {
        self.flags(l) & HOLDS != 0
    }

    /// Whether the letter silences the one before it, which so ends the
    /// syllable before (Thai's THANTHAKHAT, Lao's cancellation mark,
    /// Burmese asat and virama).
    fn silences(self, l: u8) -> bool {
        self.flags(l) & SILENCES != 0
    }

    /// Whether the sign stays with the word before it: Thai's MAIYAMOK
    /// and PAIYANNOI, Lao's KO LA and ellipsis, Khmer's repetition sign.
    fn repeats(self, l: u8) -> bool {
        self.flags(l) & REPEATS != 0
    }
}

const IN_WORD: u8 = 1;
const FOLLOWS: u8 = 2;
const LEADS: u8 = 4;
const HOLDS: u8 = 8;
const SILENCES: u8 = 16;
const REPEATS: u8 = 32;

/// Each script's letters (a table's letter, below 0x80) with their flags,
/// in [`Script::ALL`]'s order.
static FLAGS: [[u8; 0x80]; 4] = [
    flags_of(&[
        (0x01, 0x3A, IN_WORD),
        (0x40, 0x4E, IN_WORD),
        (0x2F, 0x3A, FOLLOWS),
        (0x45, 0x4E, FOLLOWS),
        (0x40, 0x44, LEADS),
        (0x31, 0x31, HOLDS),
        (0x4C, 0x4C, SILENCES),
        (0x2F, 0x2F, REPEATS),
        (0x46, 0x46, REPEATS),
    ]),
    flags_of(&[
        (0x01, 0x3D, IN_WORD),
        (0x40, 0x4E, IN_WORD),
        (0x5C, 0x5F, IN_WORD),
        (0x2F, 0x3D, FOLLOWS),
        (0x46, 0x4E, FOLLOWS),
        (0x40, 0x44, LEADS),
        (0x31, 0x31, HOLDS),
        (0x4C, 0x4C, SILENCES),
        (0x2F, 0x2F, REPEATS),
        (0x46, 0x46, REPEATS),
    ]),
    flags_of(&[
        (0x00, 0x53, IN_WORD),
        (0x57, 0x57, IN_WORD),
        (0x5D, 0x5D, IN_WORD),
        (0x34, 0x53, FOLLOWS),
        (0x57, 0x57, FOLLOWS),
        (0x5D, 0x5D, FOLLOWS),
        (0x52, 0x52, HOLDS),
        (0x57, 0x57, REPEATS),
    ]),
    flags_of(&[
        (0x00, 0x3F, IN_WORD),
        (0x4C, 0x59, IN_WORD),
        (0x2B, 0x3E, FOLLOWS),
        (0x56, 0x59, FOLLOWS),
        (0x39, 0x39, HOLDS),
        (0x39, 0x3A, SILENCES),
    ]),
];

/// A script's table from `(first, last, flag)` ranges of letters.
const fn flags_of(ranges: &[(usize, usize, u8)]) -> [u8; 0x80] {
    let mut t = [0u8; 0x80];
    let mut r = 0;
    while r < ranges.len() {
        let mut l = ranges[r].0;
        while l <= ranges[r].1 {
            t[l] |= ranges[r].2;
            l += 1;
        }
        r += 1;
    }
    t
}

/// The script of `c` if it is a letter, vowel or sign words of a script
/// written without spaces are made of.
pub(crate) fn script_of(c: char) -> Option<Script> {
    Script::ALL
        .into_iter()
        .find(|s| s.letter(c).is_some_and(|l| s.in_word(l)))
}

/// A Thai letter as the table keeps it: its code point less U+0E00.
#[cfg(test)]
fn letter(c: char) -> u8 {
    (u32::from(c) - 0xE00) as u8
}

/// The number of characters from `first` (a letter of `script`,
/// [`script_of`]) to the next place a line may break in its run (`rest`
/// the text after it), as the words of the open pack of the script say:
/// `None` without them.
pub(crate) fn word_len(script: Script, first: char, rest: &str) -> Option<u8> {
    let mut run = [0u8; WINDOW];
    run[0] = script.letter(first)?;
    let mut n = 1;
    for c in rest.chars().take(WINDOW - 1) {
        let Some(l) = script.letter(c).filter(|&l| script.in_word(l)) else {
            break;
        };
        run[n] = l;
        n += 1;
    }
    let ends = n < WINDOW;
    crate::stored::with_words(script, |words| next_place(script, &words, &run[..n], ends))
}

/// The letters of `run` (of `script`) before its first place to break (see
/// the module), at least one; `ends`: the run ends after them (else it
/// goes on past what was read).
pub(crate) fn next_place(script: Script, words: &Words<'_>, run: &[u8], ends: bool) -> u8 {
    let found = ends_at(script, words, run, ends, 0);
    // The longest word another follows, else the longest.
    for k in (1..32).rev() {
        if found >> k & 1 != 0 && (k == run.len() || ends_at(script, words, run, ends, k) != 0) {
            return k as u8;
        }
    }
    if found != 0 {
        return (31 - found.leading_zeros()) as u8;
    }
    // No word: one cluster.
    let mut at = 1;
    while at < run.len() && !place(script, run, at, ends) {
        at += 1;
    }
    at as u8
}

/// The places after the words of `run` that start at `at`: bit `k` for
/// `at + k`.
fn ends_at(script: Script, words: &Words<'_>, run: &[u8], ends: bool, at: usize) -> u32 {
    let mut set = 0;
    for len in 1..=LONGEST.min(run.len().saturating_sub(at)) {
        let (word, prefix) = words.probe(&run[at..at + len]);
        let mut end = at + len;
        while end < run.len() && end - at < 31 && script.repeats(run[end]) {
            end += 1;
        }
        if word && place(script, run, end, ends) {
            set |= 1 << (end - at);
        }
        if !prefix {
            break;
        }
    }
    set
}

/// Whether a line may break before `run[at]`; at its end if `ends`. Not
/// after MAI HAN-AKAT, whose final letter follows (or coeng), nor before a
/// letter THANTHAKHAT silences, which ends the syllable before.
#[inline(never)]
fn place(script: Script, run: &[u8], at: usize, ends: bool) -> bool {
    match (run.get(at.wrapping_sub(1)), run.get(at)) {
        (Some(&before), Some(&l)) => {
            !script.follows(l)
                && !script.leads(before)
                && !script.holds_next(before)
                && !run.get(at + 1).is_some_and(|&n| script.silences(n))
        }
        (Some(_), None) => at == run.len() && ends,
        _ => false,
    }
}

/// The words of a pack's table (slot 6). Read as they are: a table out of
/// order finds words wrongly, none outside its bytes.
#[derive(Clone, Copy)]
pub(crate) struct Words<'a> {
    count: usize,
    /// The blocks' offsets in `body`, two bytes each.
    blocks: &'a [u8],
    body: &'a [u8],
}

impl<'a> Words<'a> {
    /// The words of `table` (its head included), if its version is known
    /// and its counts are as long as it is.
    pub(crate) fn new(table: &'a [u8]) -> Option<Words<'a>> {
        let head = table.get(..HEAD)?;
        let at = |k: usize| usize::from(u16::from_le_bytes([head[k], head[k + 1]]));
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        let (count, blocks) = (at(8), at(10));
        if head[0] != VERSION || len as usize != table.len() || blocks != count.div_ceil(BLOCK) {
            return None;
        }
        let start = HEAD + 2 * blocks;
        Some(Words {
            count,
            blocks: table.get(HEAD..start)?,
            body: table.get(start..)?,
        })
    }

    /// The offset of block `b` in the body.
    fn block(&self, b: usize) -> Option<usize> {
        let bytes = self.blocks.get(2 * b..2 * b + 2)?;
        Some(usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
    }

    /// Reads the word at `at` into `word` (whose first `len` letters are
    /// the word before), the first of a block if `first`: where the next
    /// one starts.
    fn read(
        &self,
        at: usize,
        word: &mut [u8; LONGEST],
        len: &mut usize,
        first: bool,
    ) -> Option<usize> {
        let head = *self.body.get(at)?;
        let (shared, own) = (usize::from(head >> 4), usize::from(head & 15));
        if (first && shared != 0) || shared > *len || own == 0 || shared + own > LONGEST {
            return None;
        }
        word[shared..shared + own].copy_from_slice(self.body.get(at + 1..at + 1 + own)?);
        *len = shared + own;
        Some(at + 1 + own)
    }

    /// Whether `key` is a word, and whether a word starts with it.
    pub(crate) fn probe(&self, key: &[u8]) -> (bool, bool) {
        self.find(key).unwrap_or((false, false))
    }

    fn find(&self, key: &[u8]) -> Option<(bool, bool)> {
        use core::cmp::Ordering::{Equal, Greater};
        // The last block whose first word (whole, after its length) is
        // not after `key`.
        let (mut low, mut high) = (0, self.count.div_ceil(BLOCK));
        while high - low > 1 {
            let mid = (low + high) / 2;
            let at = self.block(mid)?;
            let first = self
                .body
                .get(at + 1..at + 1 + usize::from(*self.body.get(at)?))?;
            match first > key {
                true => high = mid,
                false => low = mid,
            }
        }
        // The first word not before `key`, from that block on.
        let mut word = [0u8; LONGEST];
        let (mut at, mut len) = (self.block(low)?, 0);
        for n in low * BLOCK..self.count {
            at = self.read(at, &mut word, &mut len, n % BLOCK == 0)?;
            match word[..len].cmp(key) {
                Equal => return Some((true, true)),
                Greater => return Some((false, word[..len].starts_with(key))),
                _ => {}
            }
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;

    use super::*;
    use alloc::vec::Vec;

    /// The table `tools/thai-dictionary.py` makes of `words`.
    pub(crate) fn table(words: &[&str]) -> Vec<u8> {
        table_of(Script::Thai, words)
    }

    /// The table of `words` of `script` (`tools/word-table.py`'s).
    pub(crate) fn table_of(script: Script, words: &[&str]) -> Vec<u8> {
        let mut keys: Vec<Vec<u8>> = words
            .iter()
            .map(|w| w.chars().filter_map(|c| script.letter(c)).collect())
            .collect();
        keys.sort();
        keys.dedup();
        let (mut blocks, mut body) = (Vec::new(), Vec::new());
        for (n, key) in keys.iter().enumerate() {
            let mut shared = 0;
            if n % BLOCK == 0 {
                blocks.push(body.len() as u16);
            } else {
                let before = &keys[n - 1];
                while shared < before.len()
                    && shared < key.len() - 1
                    && key[shared] == before[shared]
                {
                    shared += 1;
                }
            }
            body.push((shared << 4 | (key.len() - shared)) as u8);
            body.extend_from_slice(&key[shared..]);
        }
        let number = Script::ALL.iter().position(|&s| s == script).unwrap() as u8;
        let mut out = alloc::vec![VERSION, number, 0, 0, 0, 0, 0, 0];
        out.extend_from_slice(&(keys.len() as u16).to_le_bytes());
        out.extend_from_slice(&(blocks.len() as u16).to_le_bytes());
        for b in blocks {
            out.extend_from_slice(&b.to_le_bytes());
        }
        out.extend_from_slice(&body);
        let len = out.len() as u32;
        out[4..8].copy_from_slice(&len.to_le_bytes());
        out
    }

    /// The words of the store's table.
    pub(crate) fn dictionary() -> Vec<&'static str> {
        include_str!("../../../../tools/thai-dictionary.txt")
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    }

    /// The words of the store's Lao or Khmer table (`tools/word-table.py`).
    pub(crate) fn words_of(script: Script) -> Vec<&'static str> {
        let list = match script {
            Script::Lao => include_str!("../../../../tools/lao-words.txt"),
            Script::Myanmar => include_str!("../../../../tools/myanmar-words.txt"),
            _ => include_str!("../../../../tools/khmer-words.txt"),
        };
        list.lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    }

    /// `text` split where [`next_place`] puts its places.
    fn split(words: &Words<'_>, text: &str) -> Vec<std::string::String> {
        split_of(Script::Thai, words, text)
    }

    /// [`split`] of `text` of `script`.
    fn split_of(script: Script, words: &Words<'_>, text: &str) -> Vec<std::string::String> {
        let letters: Vec<char> = text.chars().collect();
        let (mut out, mut at) = (Vec::new(), 0);
        while at < letters.len() {
            let run: Vec<u8> = letters[at..]
                .iter()
                .take(WINDOW)
                .filter_map(|&c| script.letter(c))
                .collect();
            let ends = letters.len() - at <= WINDOW;
            let len = usize::from(next_place(script, words, &run, ends));
            out.push(letters[at..at + len].iter().collect());
            at += len;
        }
        out
    }

    #[test]
    fn the_store_table_reads_and_finds_its_words() {
        let list = dictionary();
        let made = table(&list);
        let words = Words::new(&made).expect("well made");
        for word in &list {
            let key: Vec<u8> = word.chars().map(letter).collect();
            assert_eq!(words.probe(&key), (true, true), "{word}");
        }
        // A prefix of a word only; no word.
        assert_eq!(words.probe(&[letter('ก')]), (false, true));
        assert_eq!(words.probe(&[0x4E, 0x4E]), (false, false));
    }

    #[test]
    fn a_damaged_table_is_refused_or_read_within_its_bytes() {
        let made = table(&["กด", "กดปุ่ม", "ปุ่ม"]);
        assert!(Words::new(&made).is_some());
        for cut in 0..made.len() {
            assert!(Words::new(&made[..cut]).is_none(), "{cut}");
        }
        let mut wrong = made.clone();
        wrong[0] = 2;
        assert!(Words::new(&wrong).is_none());
        // Every byte changed: refused or read without a panic.
        for at in 0..made.len() {
            let mut bytes = made.clone();
            bytes[at] ^= 0xFF;
            if let Some(words) = Words::new(&bytes) {
                let _ = split(&words, "กดปุ่มนี้");
            }
        }
    }

    #[test]
    fn a_thai_sample_breaks_between_its_words() {
        let made = table(&dictionary());
        let words = Words::new(&made).unwrap();
        // "I like to eat Thai food very much", "the government will hold a
        // meeting tomorrow", "children play in the park".
        for (text, parts) in [
            (
                "ฉันชอบกินอาหารไทยมาก",
                &["ฉัน", "ชอบ", "กิน", "อาหาร", "ไทย", "มาก"][..],
            ),
            (
                "รัฐบาลจะจัดการประชุมพรุ่งนี้",
                &["รัฐบาล", "จะ", "จัดการ", "ประชุม", "พรุ่งนี้"],
            ),
            (
                "เด็กๆเล่นในสวนสาธารณะ",
                &["เด็กๆ", "เล่น", "ใน", "สวน", "สาธารณะ"],
            ),
        ] {
            assert_eq!(split(&words, text), parts, "{text}");
        }
    }

    #[test]
    fn unknown_letters_break_between_clusters_only() {
        let made = table(&["ไทย"]);
        let words = Words::new(&made).unwrap();
        // Letters no word explains, with vowels before, above, below and
        // after their letters and tone marks: a cluster each.
        assert_eq!(
            split(&words, "เก่งที่สุดไทย"),
            ["เก่", "ง", "ที่", "สุ", "ด", "ไทย"]
        );
        for text in ["กำลังใจ", "น้ำใจ", "เปลี่ยนแปลง", "ไม้เอก", "กรุงเทพฯ"]
        {
            for part in split(&words, text) {
                let first = part.chars().next().unwrap();
                assert!(!Script::Thai.follows(letter(first)), "{text}: {part}");
                let last = part.chars().last().unwrap();
                assert!(!Script::Thai.leads(letter(last)), "{text}: {part}");
            }
        }
    }

    /// Lao and Khmer break between the words of their tables (#314): Lao's
    /// clusters as Thai's, Khmer's a consonant with those joined below it
    /// by coeng, its vowels and signs; letters no word explains, between
    /// clusters only.
    #[test]
    fn lao_and_khmer_samples_break_between_their_words() {
        let lao = table_of(Script::Lao, &words_of(Script::Lao));
        let words = Words::new(&lao).unwrap();
        // "I love the country of Laos", "people speak the Lao language".
        assert_eq!(
            split_of(Script::Lao, &words, "ຂ້ອຍຮັກປະເທດລາວ"),
            ["ຂ້ອຍ", "ຮັກ", "ປະເທດ", "ລາວ"]
        );
        assert_eq!(
            split_of(Script::Lao, &words, "ຄົນເວົ້າພາສາລາວ"),
            ["ຄົນ", "ເວົ້າ", "ພາສາ", "ລາວ"]
        );
        let khmer = table_of(Script::Khmer, &words_of(Script::Khmer));
        let words = Words::new(&khmer).unwrap();
        // "I want to go to school", "Cambodia has many people".
        assert_eq!(
            split_of(Script::Khmer, &words, "ខ្ញុំចង់ទៅសាលារៀន"),
            ["ខ្ញុំ", "ចង់", "ទៅ", "សាលារៀន"]
        );
        assert_eq!(
            split_of(Script::Khmer, &words, "ប្រទេសកម្ពុជាមានប្រជាជនច្រើន"),
            ["ប្រទេស", "កម្ពុជា", "មាន", "ប្រជាជន", "ច្រើន"]
        );
        // No word: clusters, never before a sign nor after coeng.
        let none = table_of(Script::Khmer, &["កម្ពុជា"]);
        let words = Words::new(&none).unwrap();
        assert_eq!(split_of(Script::Khmer, &words, "ស្រីខ្មែរ"), ["ស្រី", "ខ្មែ", "រ"]);
        assert_eq!(script_of('ກ'), Some(Script::Lao));
        assert_eq!(script_of('ក'), Some(Script::Khmer));
        assert_eq!(script_of('ก'), Some(Script::Thai));
        assert_eq!(script_of('\u{17D4}'), None);
        assert_eq!(script_of('\u{ED0}'), None);
    }
}
