//! PaperPaw as one of two firmwares on one X4 (#95): when a tool starts it
//! from a slot beside another firmware's, PaperPaw must never write the
//! other slot nor the OTA data that chooses between them. Phase 1 knows two
//! ways to be *declared* such a firmware, for any tool (nothing here is made
//! for one):
//!
//! - the reader writes `dual-boot = on` in the `[firmware]` section of
//!   `paperpaw.ini` (`off` says the opposite and wins; `auto`, the default,
//!   follows the marker), or
//! - the tool's bootloader carries the marker string [`MARKER`] anywhere in
//!   the bootloader's window of the flash (0 to [`WINDOW`]), or the tag
//!   [`X4BOOT`] of the X4 dualbooter's (lowio.xyz) bootloader, a tool that
//!   was made before this contract.
//!
//! [`Evidence`] is what [`decide`] weighs; a later phase adds what it
//! finds by itself (another firmware's image in the other slot, a bootloader
//! that is not the stock one) as a candidate that asks before replacing.
//! This is the part that decides, on a computer-tested footing; the firmware
//! (`shells/xteink`) reads the flash and the card and does as it says.

use alloc::string::String;
use alloc::vec::Vec;

use crate::ota::{Flash, FlashError};

/// The string a tool's bootloader carries to declare its slots managed: it
/// never writes the other slot or the OTA data. Documented for tools in
/// `docs/dual-boot.md`.
pub const MARKER: &[u8] = b"paperpaw-dualboot:1";
/// The tag inside the X4 dualbooter's bootloader (lowio.xyz, "x4boot", an
/// ESP-IDF 4.4.7 based one): it says the slots are managed by a tool that
/// knows nothing of PaperPaw. Read from the tool's page, not from its
/// binary (not available): the reader's `dual-boot = on` decides where the
/// tag is not found.
pub const X4BOOT: &[u8] = b"x4boot";
/// The bootloader's window of the flash, in bytes: the marker is looked for
/// in all of it (the partition table follows at 0x8000).
pub const WINDOW: u32 = 0x8000;
/// The section of `paperpaw.ini` and its key.
pub const SECTION: &str = "firmware";
pub const KEY: &str = "dual-boot";
/// Longest line of the file looked at; a longer one is not one of ours.
const MAX_LINE: usize = 256;

/// The code of [`InstallFailure::Other`] for an install refused because the
/// device is shared with another firmware (listed with the others in
/// `paperpaw_sdk::firmware`).
///
/// [`InstallFailure::Other`]: paperpaw_sdk::firmware::InstallFailure::Other
pub const REFUSED: u16 = 1;

/// What the reader chose in `paperpaw.ini`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Not set, or `auto`: the evidence decides.
    #[default]
    Auto,
    /// This is a dual-boot setup, whatever else is found.
    On,
    /// It is not, whatever else is found.
    Off,
}

impl Choice {
    /// The meaning of a value as the reader wrote it (`on`, `off`, `auto`,
    /// and the spellings people use for them). A value that means none of
    /// them is `On`: a typo of the guard must not leave the other firmware
    /// unguarded, and the update page names the file when it refuses. An
    /// empty value is not set.
    #[cold]
    #[inline(never)]
    pub fn of(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("auto") {
            return Self::Auto;
        }
        const OFF: [&str; 4] = ["off", "false", "no", "0"];
        if OFF.iter().any(|off| value.eq_ignore_ascii_case(off)) {
            return Self::Off;
        }
        Self::On
    }
}

/// Reads the choice from `paperpaw.ini` a piece at a time, as the card gives
/// it: the file may be large (the kernel reads up to 64 KiB) and the heap is
/// small. Lines are those of [`paperpaw_config`]'s format; the last
/// `dual-boot` of the last `[firmware]` section wins, as for any setting.
#[derive(Debug, Default)]
pub struct IniScan {
    line: Vec<u8>,
    /// The line being read is too long to be ours: skipped to its end.
    skipping: bool,
    /// The section the lines read are in is `[firmware]`.
    inside: bool,
    started: bool,
    choice: Option<Choice>,
}

impl IniScan {
    /// More of the file.
    pub fn feed(&mut self, mut bytes: &[u8]) {
        if !self.started && !bytes.is_empty() {
            self.started = true;
            // A byte order mark some editors put first.
            bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        }
        for &byte in bytes {
            match byte {
                b'\n' => self.end_line(),
                b'\r' => {}
                _ if self.skipping => {}
                _ if self.line.len() >= MAX_LINE => {
                    self.too_long();
                    self.skipping = true;
                }
                _ => self.line.push(byte),
            }
        }
    }

    /// The choice, once the whole file has been fed.
    pub fn finish(mut self) -> Choice {
        self.end_line();
        self.choice.unwrap_or_default()
    }

    /// The line being read is too long to be one of ours (a comment, say):
    /// the rest of it is skipped. What it started as still counts: a header
    /// leaves the section; a `dual-boot` entry in the section is too long
    /// to be read, and guards (the safe way to take it); any other line
    /// changes nothing.
    #[cold]
    #[inline(never)]
    fn too_long(&mut self) {
        let start = self.line.iter().position(|b| !b.is_ascii_whitespace());
        let head = start.map_or(&[][..], |at| &self.line[at..]);
        if head.first() == Some(&b'[') {
            self.inside = false;
        } else if self.inside && head.starts_with(KEY.as_bytes()) {
            self.choice = Some(Choice::On);
        }
        self.line.clear();
    }

    #[cold]
    #[inline(never)]
    fn end_line(&mut self) {
        let line = core::mem::take(&mut self.line);
        if core::mem::take(&mut self.skipping) {
            return;
        }
        let Ok(line) = core::str::from_utf8(&line) else {
            return;
        };
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            self.inside = header
                .split_once(']')
                .is_some_and(|(name, _)| name.trim() == SECTION);
            return;
        }
        if !self.inside || line.is_empty() {
            return;
        }
        // The line read as the kernel reads it: by the parser of the file's
        // format (quotes, comments, escapes), in the section tracked here.
        for entry in paperpaw_config::parse(line).flatten() {
            if entry.key == KEY {
                self.choice = Some(Choice::of(&entry.value.as_str()));
            }
        }
    }
}

/// Looks for a marker ([`MARKER`] by default) in a flash read a piece at a
/// time (a piece may end inside it).
#[derive(Debug)]
pub struct MarkerScan {
    needle: &'static [u8],
    /// How much of the marker the last bytes read are the start of.
    matched: usize,
}

impl Default for MarkerScan {
    fn default() -> Self {
        Self::of(MARKER)
    }
}

impl MarkerScan {
    /// A look for `needle`.
    pub const fn of(needle: &'static [u8]) -> Self {
        Self { needle, matched: 0 }
    }

    /// More of the flash. Whether the marker has been seen by now.
    pub fn feed(&mut self, piece: &[u8]) -> bool {
        let needle = self.needle;
        for &byte in piece {
            // On a mismatch, fall back to the longest start of the marker
            // that the bytes before it end in, and try again.
            while self.matched > 0 && needle[self.matched] != byte {
                let m = self.matched;
                self.matched = (0..m)
                    .rev()
                    .find(|&n| needle[..n] == needle[m - n..m])
                    .unwrap_or(0);
            }
            if needle[self.matched] == byte {
                self.matched += 1;
                if self.matched == needle.len() {
                    return true;
                }
            }
        }
        false
    }
}

/// Which marker the bootloader's window of `flash` carries, if one
/// ([`MARKER`], else [`X4BOOT`]), read in pieces of 1 KB (about 32 KB in
/// all, so only where it is needed; both are looked for in the same pass).
#[cold]
#[inline(never)]
pub fn scan_marker(flash: &mut impl Flash) -> Result<Option<Source>, FlashError> {
    let mut piece = [0u8; 1024];
    let mut ours = MarkerScan::of(MARKER);
    let mut lowio = MarkerScan::of(X4BOOT);
    let mut tag = false;
    for at in (0..WINDOW).step_by(piece.len()) {
        flash.read(at, &mut piece)?;
        if ours.feed(&piece) {
            return Ok(Some(Source::Marker));
        }
        // Ours wins where both are there: the whole window is read for it.
        tag = lowio.feed(&piece) || tag;
    }
    Ok(tag.then_some(Source::X4Boot))
}

/// What PaperPaw knows of its setup.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Evidence {
    /// The reader's choice in `paperpaw.ini`.
    pub choice: Choice,
    /// The marker the bootloader carries ([`scan_marker`]); `None` where it
    /// carries none or was not looked for.
    pub marker: Option<Source>,
}

/// Why the setup is a dual-boot one, for the note the plugins read and the
/// log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `dual-boot = on` in `paperpaw.ini`.
    Ini,
    /// The marker in the bootloader.
    Marker,
    /// The tag of the X4 dualbooter's bootloader ([`X4BOOT`]).
    X4Boot,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Ini => "ini",
            Source::Marker => "marker",
            Source::X4Boot => "x4boot",
        }
    }
}

/// Whether this is a declared dual-boot setup, and by what. `Off` wins over
/// everything; `On` over the marker. Where nothing declares it, `None`: a
/// later phase adds what PaperPaw finds out by itself.
pub fn decide(evidence: Evidence) -> Option<Source> {
    match evidence.choice {
        Choice::Off => None,
        Choice::On => Some(Source::Ini),
        Choice::Auto => evidence.marker,
    }
}

/// Whether the marker has to be looked for: not where the reader decided.
pub fn needs_marker(choice: Choice) -> bool {
    choice == Choice::Auto
}

/// The file the firmware leaves on the card for plugins to read
/// (`.paperpaw/boot.txt`, one line), where a dual-boot setup is declared;
/// removed where it is not.
pub const NOTE: &str = ".paperpaw/boot.txt";

/// The note's text: `dual-boot source=marker slot=1` (the slot counts from
/// 0, the one running; `slot=?` where which runs could not be told).
pub fn note(source: Source, slot: Option<u32>) -> String {
    match slot {
        Some(slot) => alloc::format!("dual-boot source={} slot={slot}\n", source.name()),
        None => alloc::format!("dual-boot source={} slot=?\n", source.name()),
    }
}

/// Whether a note's text says a dual-boot setup is declared (what plugins
/// check; anything else, a cut file included, says it is not).
pub fn noted(text: &str) -> bool {
    text.lines()
        .next()
        .is_some_and(|line| line.split_whitespace().next() == Some("dual-boot"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(text: &str) -> Choice {
        let mut scan = IniScan::default();
        scan.feed(text.as_bytes());
        scan.finish()
    }

    #[test]
    fn the_values_mean_what_people_mean() {
        for on in ["on", "ON", "true", "yes", "1"] {
            assert_eq!(Choice::of(on), Choice::On, "{on}");
        }
        for off in ["off", "Off", "false", "no", "0", "  off "] {
            assert_eq!(Choice::of(off), Choice::Off, "{off}");
        }
        for auto in ["auto", "AUTO", "", "   "] {
            assert_eq!(Choice::of(auto), Choice::Auto, "{auto:?}");
        }
        // A typo guards, and does not leave the other firmware open.
        assert_eq!(Choice::of("onn"), Choice::On);
        assert_eq!(Choice::of("maybe"), Choice::On);
    }

    #[test]
    fn the_choice_is_read_from_its_section() {
        assert_eq!(choice("[firmware]\ndual-boot = on\n"), Choice::On);
        assert_eq!(choice("[firmware]\ndual-boot = off\n"), Choice::Off);
        assert_eq!(choice("[firmware]\ndual-boot = auto\n"), Choice::Auto);
        assert_eq!(choice(""), Choice::Auto);
        assert_eq!(choice("[plugins]\nstats = off\n"), Choice::Auto);
        // Another section's key of the same name is not ours.
        assert_eq!(choice("[mail]\ndual-boot = on\n"), Choice::Auto);
        assert_eq!(choice("dual-boot = on\n"), Choice::Auto);
    }

    #[test]
    fn the_last_of_the_file_wins_across_sections() {
        let text =
            "[firmware]\ndual-boot = on\n[core]\nlanguage = de\n[firmware]\ndual-boot = off\n";
        assert_eq!(choice(text), Choice::Off);
        let text = "[firmware]\ndual-boot = off\ndual-boot = on\n";
        assert_eq!(choice(text), Choice::On);
    }

    #[test]
    fn spacing_comments_quotes_and_line_ends_are_the_files_own() {
        assert_eq!(choice("\u{feff}[firmware]\r\ndual-boot=on\r\n"), Choice::On);
        assert_eq!(
            choice("  [ firmware ]  \n  dual-boot   =   on  \n"),
            Choice::On
        );
        assert_eq!(choice("[firmware]\ndual-boot = \"on\"\n"), Choice::On);
        assert_eq!(
            choice("[firmware]\ndual-boot = on # I use two\n"),
            Choice::On
        );
        assert_eq!(choice("[firmware]\n# dual-boot = on\n"), Choice::Auto);
        assert_eq!(choice("[firmware]\ndual-boot = on"), Choice::On);
    }

    #[test]
    fn a_damaged_file_never_panics_and_decides_nothing_it_cannot_read() {
        // Not UTF-8, a header cut short, a very long line, no newline, NULs.
        assert_eq!(
            choice("[firmware]\n\u{0}\u{0}dual-boot = on\n"),
            Choice::Auto
        );
        let mut bytes = b"[firmware]\n\xff\xfe\ndual-boot = on\n".to_vec();
        let mut scan = IniScan::default();
        scan.feed(&bytes);
        assert_eq!(scan.finish(), Choice::On);
        bytes = b"[firmwa".to_vec();
        let mut scan = IniScan::default();
        scan.feed(&bytes);
        assert_eq!(scan.finish(), Choice::Auto);
        let long = alloc::format!("[firmware]\n{}\ndual-boot = on\n", "x".repeat(10_000));
        assert_eq!(choice(&long), Choice::On);
        // A bare key without a value, an equals sign alone.
        assert_eq!(choice("[firmware]\ndual-boot\n="), Choice::Auto);
        assert_eq!(choice("[firmware\ndual-boot = on\n"), Choice::Auto);
    }

    #[test]
    fn a_long_line_changes_what_it_is_and_no_more() {
        let long = alloc::format!("# {}", "x".repeat(2000));
        // A long comment in the section does not turn the guard off.
        assert_eq!(
            choice(&alloc::format!("[firmware]\n{long}\ndual-boot = on\n")),
            Choice::On
        );
        assert_eq!(
            choice(&alloc::format!("[firmware]\ndual-boot = on\n{long}\n")),
            Choice::On
        );
        // A long line of another key: no change either.
        let key = alloc::format!("[firmware]\nnote = {}\ndual-boot = off\n", "y".repeat(500));
        assert_eq!(choice(&key), Choice::Off);
        // A header too long to be read leaves the section.
        let header = alloc::format!("[{}]\ndual-boot = on\n", "z".repeat(500));
        assert_eq!(
            choice(&alloc::format!("[firmware]\n{header}")),
            Choice::Auto
        );
        // A `dual-boot` entry too long to be read guards: the safe way.
        let entry = alloc::format!("[firmware]\ndual-boot = off {}\n", "w".repeat(500));
        assert_eq!(choice(&entry), Choice::On);
        // Outside the section it is not ours.
        let outside = alloc::format!("[core]\ndual-boot = off {}\n", "w".repeat(500));
        assert_eq!(choice(&outside), Choice::Auto);
        // Indented, and in any pieces.
        let mut scan = IniScan::default();
        for piece in alloc::format!("[firmware]\n  {long}\ndual-boot=on")
            .as_bytes()
            .chunks(7)
        {
            scan.feed(piece);
        }
        assert_eq!(scan.finish(), Choice::On);
    }

    #[test]
    fn the_file_may_come_in_any_pieces() {
        let text = b"[core]\nx = 1\n[firmware]\ndual-boot = on\n[core]\ny = 2\n";
        for size in [1, 2, 3, 7, 16, 100] {
            let mut scan = IniScan::default();
            for piece in text.chunks(size) {
                scan.feed(piece);
            }
            assert_eq!(scan.finish(), Choice::On, "pieces of {size}");
        }
    }

    #[test]
    fn the_marker_is_found_in_any_pieces_and_only_whole() {
        let mut flash = alloc::vec![0xFFu8; 0x400];
        flash[0x3F0 - 10..0x3F0 - 10 + MARKER.len()].copy_from_slice(MARKER);
        for size in [1, 5, 19, 64, 4096] {
            let mut scan = MarkerScan::default();
            let found = flash.chunks(size).any(|piece| scan.feed(piece));
            assert!(found, "pieces of {size}");
        }
        // The dualbooter's tag, the same way.
        let mut flash = alloc::vec![0xFFu8; 0x400];
        flash[0x3F0 - 3..0x3F0 - 3 + X4BOOT.len()].copy_from_slice(X4BOOT);
        for size in [1, 2, 5, 64, 4096] {
            let mut scan = MarkerScan::of(X4BOOT);
            let found = flash.chunks(size).any(|piece| scan.feed(piece));
            assert!(found, "x4boot in pieces of {size}");
        }
        // Cut short, or another version, is not it.
        for text in [
            &MARKER[..MARKER.len() - 1],
            b"paperpaw-dualboot:2".as_slice(),
        ] {
            let mut scan = MarkerScan::default();
            assert!(!scan.feed(text));
            assert!(!scan.feed(&[0xFF; 64]));
        }
    }

    #[test]
    fn a_false_start_of_the_marker_does_not_hide_it() {
        // Bytes that begin like the marker, then the marker itself.
        for lead in [
            "p",
            "pap",
            "paperp",
            "paperpaw-dualboot:",
            "paperpaw-dualboot",
        ] {
            let mut flash = lead.as_bytes().to_vec();
            flash.extend_from_slice(MARKER);
            for size in [1, 3, 64] {
                let mut scan = MarkerScan::default();
                let found = flash.chunks(size).any(|piece| scan.feed(piece));
                assert!(found, "{lead} in pieces of {size}");
            }
        }
    }

    #[test]
    fn what_declares_the_setup_and_what_overrules() {
        let ev = |choice, marker| Evidence { choice, marker };
        assert_eq!(decide(ev(Choice::Auto, None)), None);
        assert_eq!(
            decide(ev(Choice::Auto, Some(Source::Marker))),
            Some(Source::Marker)
        );
        assert_eq!(
            decide(ev(Choice::Auto, Some(Source::X4Boot))),
            Some(Source::X4Boot)
        );
        assert_eq!(decide(ev(Choice::On, None)), Some(Source::Ini));
        assert_eq!(
            decide(ev(Choice::On, Some(Source::X4Boot))),
            Some(Source::Ini)
        );
        // Off wins over the marker.
        assert_eq!(decide(ev(Choice::Off, Some(Source::Marker))), None);
        assert_eq!(decide(ev(Choice::Off, Some(Source::X4Boot))), None);
        assert!(needs_marker(Choice::Auto));
        assert!(!needs_marker(Choice::On) && !needs_marker(Choice::Off));
    }

    #[test]
    fn the_note_says_it_and_a_cut_one_does_not() {
        let text = note(Source::Marker, Some(1));
        assert_eq!(text, "dual-boot source=marker slot=1\n");
        let text = note(Source::X4Boot, None);
        assert_eq!(text, "dual-boot source=x4boot slot=?\n");
        assert!(noted(&text));
        let text = note(Source::Marker, Some(1));
        assert!(noted(&text));
        assert!(noted("dual-boot"));
        assert!(!noted(""));
        assert!(!noted("dualboot source=ini"));
        assert!(!noted("\n dual-boot"));
    }
}
