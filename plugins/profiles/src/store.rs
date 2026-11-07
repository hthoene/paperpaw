//! The file of profiles (`profiles.ini`, version 2) and what a profile's
//! values are, without the host. Every growth is reserved first: a refused
//! allocation is `Error::NoMemory` and leaves the caller's state as it was.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Id, IdError, config, native::Error};
use paperpaw_ui_kit::{
    mem::push,
    text::{cmp_lowercase, concat, copy},
};

/// A named set of setting values.
#[derive(Debug, PartialEq, Eq)]
pub struct Profile {
    /// Its name.
    pub name: String,
    /// Setting ids and values as in `paperpaw.ini`.
    pub values: Vec<(String, String)>,
}

impl Profile {
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.values.len())
            .map_err(|_| Error::NoMemory)?;
        for (id, value) in &self.values {
            values.push((copy(id)?, copy(value)?));
        }
        Ok(Self {
            name: copy(&self.name)?,
            values,
        })
    }
}

/// The file's version this plugin writes.
const VERSION: u32 = 2;

/// A string grown only after its room is reserved.
struct Out(String);

impl Out {
    fn put(&mut self, text: &str) -> Result<(), Error> {
        self.0
            .try_reserve(text.len())
            .map_err(|_| Error::NoMemory)?;
        self.0.push_str(text);
        Ok(())
    }

    fn put_char(&mut self, c: char) -> Result<(), Error> {
        self.0
            .try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        self.0.push(c);
        Ok(())
    }

    /// `value` as `config::quote` writes it: quoted and escaped where it would
    /// not survive unquoted.
    fn put_quoted(&mut self, value: &str) -> Result<(), Error> {
        let plain = !value.is_empty()
            && value.trim() == value
            && !value.starts_with('"')
            && !value.contains(['\n', '\r']);
        if plain {
            return self.put(value);
        }
        self.put_char('"')?;
        for c in value.chars() {
            match c {
                '"' => self.put("\\\"")?,
                '\\' => self.put("\\\\")?,
                '\n' => self.put("\\n")?,
                '\t' => self.put("\\t")?,
                '\r' => {}
                c => self.put_char(c)?,
            }
        }
        self.put_char('"')
    }

    fn put_number(&mut self, number: u64) -> Result<(), Error> {
        let mut digits = [0; 20];
        self.put(paperpaw_ui_kit::text::decimal_u64(number, &mut digits))
    }
}

/// Version 1 to 2: the names out of the section headers (version 1 had the
/// name as the section, `[Night]`, which an ini file cannot hold with spaces).
fn upgrade_names(v1: &str) -> Result<String, Error> {
    let mut profiles: Vec<Profile> = Vec::new();
    for line in v1.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            push(
                &mut profiles,
                Profile {
                    name: copy(name.trim())?,
                    values: Vec::new(),
                },
            )?;
        } else if let (Some(profile), Some((id, value))) =
            (profiles.last_mut(), line.split_once('='))
            && is_id(id.trim())?
        {
            push(&mut profile.values, (copy(id.trim())?, copy(value.trim())?))?;
        }
    }
    text(&profiles)
}

/// Whether `id` is a setting id, by the SDK's own check (`Id::parse`): the same ids
/// interface 6 kept, such as `widgets.clock@2.face`.
///
/// # Errors
/// [`Error::NoMemory`] where the check could not copy the id.
pub fn is_id(id: &str) -> Result<bool, Error> {
    match Id::try_parse(id) {
        Ok(_) => Ok(true),
        Err(IdError::Invalid) => Ok(false),
        Err(_) => Err(Error::NoMemory),
    }
}

/// The profiles as the file's text (without its version).
fn text(profiles: &[Profile]) -> Result<String, Error> {
    let mut out = Out(String::new());
    for (i, profile) in profiles.iter().enumerate() {
        if i > 0 {
            out.put_char('\n')?;
        }
        out.put("[profile.")?;
        out.put_number(i as u64 + 1)?;
        out.put("]\nname = ")?;
        out.put_quoted(&profile.name)?;
        out.put_char('\n')?;
        for (id, value) in &profile.values {
            out.put(id)?;
            out.put(" = ")?;
            out.put_quoted(value)?;
            out.put_char('\n')?;
        }
    }
    Ok(out.0)
}

/// The file's text for `profiles`: its version, then the profiles.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn encode(profiles: &[Profile]) -> Result<String, Error> {
    encode_within(profiles, usize::MAX)
}

/// The file's text for `profiles`, refused when it would be longer than `limit`
/// bytes: what is written must be what [`decode`]'s reader accepts.
///
/// # Errors
/// [`Error::NoSpace`] beyond `limit`, [`Error::NoMemory`].
pub fn encode_within(profiles: &[Profile], limit: usize) -> Result<String, Error> {
    let body = text(profiles)?;
    let mut digits = [0; 20];
    let head = concat(&[
        "# format ",
        paperpaw_ui_kit::text::decimal(VERSION as usize, &mut digits),
        "\n",
    ])?;
    let len = head.len().checked_add(body.len()).ok_or(Error::NoSpace)?;
    if len > limit {
        return Err(Error::NoSpace);
    }
    let mut out = Out(head);
    out.put(&body)?;
    Ok(out.0)
}

/// The profiles of a file; `None` where it cannot be read as one (a newer
/// version, or not text): then nothing may be saved over it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn decode(bytes: &[u8]) -> Result<Option<Vec<Profile>>, Error> {
    let bytes = without_bom(bytes);
    let Ok(source) = core::str::from_utf8(bytes) else {
        return Ok(None);
    };
    let version = source
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("# format "))
        .map_or(Some(1), |v| v.trim().parse::<u32>().ok());
    match version {
        Some(1) => parse(&upgrade_names(source)?).map(Some),
        Some(VERSION) => parse(source).map(Some),
        _ => Ok(None),
    }
}

/// The UTF-8 byte order mark an editor may put in front of a hand-edited file, which
/// interface 6 skipped too.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// How many bytes at the start of the file [`may_save_over`] needs.
pub const HEAD: usize = 64;

fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(BOM).unwrap_or(bytes)
}

/// Whether the start of a file allows a save over it: a file of this version or the
/// first one, or one with no version line. A newer version is left alone.
///
/// `head` is the file's first [`HEAD`] bytes (fewer where the file is shorter).
#[must_use]
pub fn may_save_over(head: &[u8]) -> bool {
    let full = head.len() >= HEAD;
    let head = without_bom(head);
    // A version line that does not end within the bytes is not one this version
    // knows. A first line of the first version (`[A long profile name]`) is no
    // version line at all and may be longer.
    if full && !head.contains(&b'\n') && head.starts_with(b"# format ") {
        return false;
    }
    // A cut in the middle of a character is no reason to refuse.
    let text = match core::str::from_utf8(head) {
        Ok(text) => text,
        Err(e) => core::str::from_utf8(&head[..e.valid_up_to()]).unwrap_or(""),
    };
    match text
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("# format "))
    {
        None => true,
        Some(v) => matches!(v.trim().parse::<u32>(), Ok(1 | VERSION)),
    }
}

/// A value's text with escapes resolved, as `config::Value::as_str` gives it.
fn value_text(source: &str, value: config::Value<'_>) -> Result<String, Error> {
    let raw = value.raw();
    // A quoted value is the text between quotes: the byte before it is the quote.
    let at = (raw.as_ptr() as usize).wrapping_sub(source.as_ptr() as usize);
    let quoted = at > 0 && at <= source.len() && source.as_bytes()[at - 1] == b'"';
    if !quoted || !raw.contains('\\') {
        return copy(raw);
    }
    let mut out = Out(String::new());
    out.0
        .try_reserve_exact(raw.len())
        .map_err(|_| Error::NoMemory)?;
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.put_char(c)?;
            continue;
        }
        match chars.next() {
            Some('n') => out.put_char('\n')?,
            Some('t') => out.put_char('\t')?,
            Some('u') => {
                let rest = chars.as_str();
                if let Some((c, len)) = unicode_escape(rest) {
                    out.put_char(c)?;
                    chars = rest[len..].chars();
                }
            }
            Some(other) => out.put_char(other)?,
            None => {}
        }
    }
    Ok(out.0)
}

/// `{XXXX}` (1 to 6 hex digits): the character and the bytes it took.
fn unicode_escape(s: &str) -> Option<(char, usize)> {
    let body = s.strip_prefix('{')?;
    let end = body.find('}')?;
    let hex = &body[..end];
    if hex.is_empty() || hex.len() > 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let code = u32::from_str_radix(hex, 16).ok()?;
    Some((char::from_u32(code)?, end + 2))
}

/// The profiles of the file's text (in this version).
fn parse(text: &str) -> Result<Vec<Profile>, Error> {
    let mut profiles: Vec<(&str, Profile)> = Vec::new();
    for entry in config::parse(text).filter_map(Result::ok) {
        if !entry.section.starts_with("profile.") {
            continue;
        }
        if profiles.last().is_none_or(|(s, _)| *s != entry.section) {
            push(
                &mut profiles,
                (
                    entry.section,
                    Profile {
                        name: String::new(),
                        values: Vec::new(),
                    },
                ),
            )?;
        }
        let Some((_, profile)) = profiles.last_mut() else {
            continue;
        };
        if entry.key == "name" {
            profile.name = value_text(text, entry.value)?;
        } else if is_id(entry.key)? {
            let value = value_text(text, entry.value)?;
            push(&mut profile.values, (copy(entry.key)?, value))?;
        }
    }
    let mut found = Vec::new();
    found
        .try_reserve_exact(profiles.len())
        .map_err(|_| Error::NoMemory)?;
    found.extend(profiles.into_iter().map(|(_, p)| p));
    Ok(found)
}

/// Whether `a` and `b` name the same profile: as they read, whatever the case.
#[must_use]
pub fn same_name(a: &str, b: &str) -> bool {
    cmp_lowercase(a, b).is_eq()
}

/// A name as typed: words joined by single spaces, without brackets; `None`
/// where nothing is left.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn clean(name: &str) -> Result<Option<String>, Error> {
    let mut out = Out(String::new());
    out.0
        .try_reserve_exact(name.len())
        .map_err(|_| Error::NoMemory)?;
    for (at, word) in name.split_whitespace().enumerate() {
        if at > 0 {
            out.put_char(' ')?;
        }
        for c in word.chars().filter(|c| !matches!(c, '[' | ']')) {
            out.put_char(c)?;
        }
    }
    let name = out.0.trim_end().len();
    out.0.truncate(name);
    let start = out.0.len() - out.0.trim_start().len();
    out.0.drain(..start);
    Ok((!out.0.is_empty()).then_some(out.0))
}

/// The kind of value a setting holds, as far as profiles care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// On or off.
    Bool,
    /// A whole number between `min` and `max`.
    Int { min: i64, max: i64 },
    /// One of a list of ids.
    Choice,
}

/// A value of the kind in the text of a configuration file, as the profile
/// keeps it, or in the typed form: `None` where the setting does not take it.
#[derive(Debug, PartialEq, Eq)]
pub enum Value {
    /// A switch.
    Bool(bool),
    /// A number.
    Int(i64),
    /// A choice.
    Text(String),
}

impl Value {
    /// How a profile keeps it.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn to_text(&self) -> Result<String, Error> {
        match self {
            Self::Bool(true) => copy("on"),
            Self::Bool(false) => copy("off"),
            Self::Int(v) => {
                let mut out = Out(String::new());
                if *v < 0 {
                    out.put_char('-')?;
                }
                out.put_number(v.unsigned_abs())?;
                Ok(out.0)
            }
            Self::Text(v) => copy(v),
        }
    }

    /// Whether [`Self::to_text`] would be `text`, without making it.
    #[must_use]
    pub fn text_is(&self, text: &str) -> bool {
        match self {
            Self::Bool(true) => text == "on",
            Self::Bool(false) => text == "off",
            Self::Int(v) => {
                let mut digits = [0; 20];
                let magnitude = paperpaw_ui_kit::text::decimal_u64(v.unsigned_abs(), &mut digits);
                match text.strip_prefix('-') {
                    Some(rest) => *v < 0 && rest == magnitude,
                    None => *v >= 0 && text == magnitude,
                }
            }
            Self::Text(v) => text == v,
        }
    }

    /// The value `text` is for a setting of `kind`, if it takes it.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn parse(kind: Kind, text: &str) -> Result<Option<Self>, Error> {
        Ok(match kind {
            Kind::Bool => {
                let word = text.trim();
                let is = |names: &[&str]| names.iter().any(|n| word.eq_ignore_ascii_case(n));
                if is(&["on", "true", "yes", "1"]) {
                    Some(Self::Bool(true))
                } else if is(&["off", "false", "no", "0"]) {
                    Some(Self::Bool(false))
                } else {
                    None
                }
            }
            Kind::Int { min, max } => text
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|v| (min..=max).contains(v))
                .map(Self::Int),
            Kind::Choice if text.is_empty() => None,
            Kind::Choice => Some(Self::Text(copy(text)?)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> [Profile; 2] {
        [
            Profile {
                name: "Night".into(),
                values: Vec::from([
                    ("core.night-mode".into(), "on".into()),
                    ("core.font-size".into(), "12".into()),
                ]),
            },
            Profile {
                name: "Large [print]".into(),
                values: Vec::from([("core.font-size".into(), "18".into())]),
            },
        ]
    }

    #[test]
    fn profiles_are_kept_whatever_their_names() {
        assert!(decode(b"").unwrap().unwrap().is_empty());
        let profiles = profiles();
        let loaded = decode(encode(&profiles).unwrap().as_bytes())
            .unwrap()
            .unwrap();
        assert_eq!(loaded, profiles, "names with brackets and spaces too");
    }

    #[test]
    fn the_file_of_version_2_is_read_as_it_was_written() {
        let loaded = decode(include_bytes!("../testdata/profiles.ini.v2"))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].name, "Night reading");
        assert_eq!(loaded[1].name, "  Large [print] = big ");
        assert_eq!(loaded[2].name, "Quote \" and \\ back");
        assert_eq!(
            loaded[2].values,
            [("core.line-spacing".into(), String::new())]
        );
        // What this writes is what it read, byte for byte.
        assert_eq!(
            encode(&loaded).unwrap().as_bytes(),
            include_bytes!("../testdata/profiles.ini.v2")
        );
    }

    #[test]
    fn files_of_version_1_are_read_and_written_as_version_2() {
        let loaded = decode(include_bytes!("../testdata/profiles.ini.v1"))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Night");
        assert_eq!(
            loaded[0].values,
            [
                ("core.night-mode".into(), "on".into()),
                ("core.font-size".into(), "12".into()),
            ]
        );
        assert_eq!(loaded[1].name, "Large print");
        let text = encode(&loaded).unwrap();
        assert!(text.starts_with("# format 2\n[profile.1]\nname = Night\n"));
        assert_eq!(decode(text.as_bytes()).unwrap().unwrap(), loaded);
    }

    #[test]
    fn a_file_of_another_version_or_not_text_is_not_read() {
        assert_eq!(
            decode(b"# format 3\n[profile.1]\nname = x\n").unwrap(),
            None
        );
        assert_eq!(decode(b"# format 0\n").unwrap(), None);
        assert_eq!(decode(b"# format x\n").unwrap(), None);
        assert_eq!(decode(&[0xff, 0xfe]).unwrap(), None);
    }

    #[test]
    fn names_are_tidied_and_compared_without_case() {
        assert_eq!(
            clean("  Night   [mode] ").unwrap(),
            Some("Night mode".into())
        );
        assert_eq!(clean(" [ ] ").unwrap(), None);
        assert!(same_name("NIGHT", "night"));
        assert!(!same_name("Night", "Nights"));
    }

    #[test]
    fn values_are_taken_only_where_the_setting_accepts_them() {
        let int = Kind::Int { min: 6, max: 40 };
        assert_eq!(
            Value::parse(Kind::Bool, "on").unwrap(),
            Some(Value::Bool(true))
        );
        assert_eq!(
            Value::parse(Kind::Bool, "Off").unwrap(),
            Some(Value::Bool(false))
        );
        assert_eq!(Value::parse(Kind::Bool, "maybe").unwrap(), None);
        assert_eq!(Value::parse(int, "12").unwrap(), Some(Value::Int(12)));
        assert_eq!(Value::parse(int, "41").unwrap(), None);
        assert_eq!(Value::parse(int, "x").unwrap(), None);
        assert_eq!(
            Value::parse(Kind::Choice, "core.contrast.strong").unwrap(),
            Some(Value::Text("core.contrast.strong".into()))
        );
        assert_eq!(Value::parse(Kind::Choice, "").unwrap(), None);
        assert_eq!(Value::Bool(true).to_text().unwrap(), "on");
        assert_eq!(Value::Int(-3).to_text().unwrap(), "-3");
    }

    #[test]
    fn a_leading_byte_order_mark_is_skipped_in_every_version() {
        let v2 = "\u{feff}# format 2\n[profile.1]\nname = Night Reading\ncore.night-mode = on\n";
        let loaded = decode(v2.as_bytes()).unwrap().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Night Reading");
        assert_eq!(loaded[0].values, [("core.night-mode".into(), "on".into())]);
        // Version 1 (the section is the name), and a file that is only the mark.
        let v1 = "\u{feff}[Night]\ncore.night-mode = on\n";
        let loaded = decode(v1.as_bytes()).unwrap().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Night");
        assert!(decode("\u{feff}".as_bytes()).unwrap().unwrap().is_empty());
        // What is written back has no mark and reads the same.
        let again = encode(&loaded).unwrap();
        assert!(again.starts_with("# format 2\n"));
        assert_eq!(decode(again.as_bytes()).unwrap().unwrap(), loaded);
        // A newer version behind the mark is still left alone.
        assert_eq!(decode("\u{feff}# format 3\n".as_bytes()), Ok(None));
    }

    #[test]
    fn ids_with_placements_survive_a_round_trip() {
        for id in ["widgets.clock@2.face", "core.font-size", "a.b@10", "x"] {
            assert_eq!(is_id(id), Ok(true), "{id}");
        }
        for id in [
            "", "Core.x", "a..b", "a@", "a@x", ".a", "a.", "a b.c", "a=b.c",
        ] {
            assert_eq!(is_id(id), Ok(false), "{id:?}");
        }
        let profiles = [Profile {
            name: "Clock".into(),
            values: Vec::from([
                ("widgets.clock@2.face".into(), "round".into()),
                ("core.night-mode".into(), "on".into()),
            ]),
        }];
        let text = encode(&profiles).unwrap();
        assert_eq!(decode(text.as_bytes()).unwrap().unwrap(), profiles);
        // The same in a file of the first version.
        let v1 = "[Clock]\nwidgets.clock@2.face = round\ncore.night-mode = on\n";
        assert_eq!(decode(v1.as_bytes()).unwrap().unwrap(), profiles);
    }

    #[test]
    fn damaged_files_give_what_can_be_read_or_nothing_and_never_panic() {
        for text in [
            &b""[..],
            b"# format 2\n",
            b"# format 2",
            b"# format 2\n[profile.1]\nname = \"unclosed\n",
            b"# format 2\n[profile.1\nname = x\n",
            b"# format 2\nname = outside\n[other]\nname = not a profile\n",
            b"# format\n[x]\n",
            b"# format 999999999999999999999\n[profile.1]\nname = x\n",
            b"[[[\n]]]\n= =\n",
            b"\xEF\xBB",
            b"\xEF\xBB\xBF\xEF\xBB\xBF# format 2\n[profile.1]\nname = x\n",
        ] {
            let _ = decode(text);
        }
        let got = decode(b"# format 2\n[other]\nname = no\n[profile.9]\nname = yes\n")
            .unwrap()
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "yes");
        // Not UTF-8, or a newer version: not read, so nothing is saved over it.
        assert_eq!(
            decode(b"# format 2\n[profile.1]\nname = \xff\xfe\n"),
            Ok(None)
        );
        assert_eq!(decode(b"# format 3\n[set.1]\ntitle = x\n"), Ok(None));
        // A profile without a name line keeps its values.
        let got = decode(b"# format 2\n[profile.1]\ncore.night-mode = on\nname = Later\n")
            .unwrap()
            .unwrap();
        assert_eq!(got[0].name, "Later");
        assert_eq!(got[0].values.len(), 1);
    }

    #[test]
    fn only_a_readable_version_is_saved_over() {
        assert!(may_save_over(b""));
        assert!(may_save_over(b"# format 2\n[profile.1]\n"));
        assert!(may_save_over(b"# format 1\n"));
        assert!(may_save_over(b"[Night]\ncore.x = 1\n"));
        assert!(may_save_over("\u{feff}# format 2\n".as_bytes()));
        assert!(!may_save_over(b"# format 3\n[set.1]\n"));
        assert!(!may_save_over(b"# format 0\n"));
        assert!(!may_save_over(b"# format x\n"));
        assert!(!may_save_over("\u{feff}# format 3\n".as_bytes()));
        // A version line that fills the bytes read is not one that is known.
        let mut long = Vec::from(&b"# format 2"[..]);
        long.resize(HEAD, b' ');
        assert!(!may_save_over(&long));
        // A first-version file whose first profile has a long name has no version
        // line: it may be saved over, with or without a mark, however long the line.
        let mut v1 = Vec::from(&b"["[..]);
        v1.resize(HEAD, b'n');
        assert!(may_save_over(&v1));
        let mut marked = Vec::from("\u{feff}[".as_bytes());
        marked.resize(HEAD, b'n');
        assert!(may_save_over(&marked));
        // A cut inside a character is no reason to refuse a good file.
        let mut cut = Vec::from("# format 2\n".as_bytes());
        cut.extend_from_slice(&"é".as_bytes()[..1]);
        assert!(may_save_over(&cut));
    }

    #[test]
    fn the_limit_is_checked_before_anything_is_written() {
        let profiles = profiles();
        let size = encode(&profiles).unwrap().len();
        assert_eq!(encode_within(&profiles, size).unwrap().len(), size);
        assert_eq!(encode_within(&profiles, size - 1), Err(Error::NoSpace));
    }

    #[test]
    fn numbers_keep_their_text_at_the_ends_of_the_range() {
        for v in [0, -1, 7, i64::MIN, i64::MAX] {
            let value = Value::Int(v);
            let text = value.to_text().unwrap();
            assert_eq!(text, alloc::format!("{v}"));
            assert!(value.text_is(&text));
            assert!(!value.text_is(&alloc::format!(" {text}")));
        }
        assert!(!Value::Int(5).text_is("-5"));
        assert!(!Value::Int(-5).text_is("5"));
    }

    mod against_the_config_crate {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// What is written and read back is what the standard quoting and
            /// unescaping make of any value.
            #[test]
            fn values_are_written_and_read_as_the_config_crate_does(
                name in "[ -~\\n\\t\\r\u{e4}\u{1f600}]{0,24}",
                value in "[ -~\\n\\t\\r\u{e4}\u{1f600}]{0,24}",
            ) {
                let profile = Profile {
                    name: name.clone(),
                    values: Vec::from([(String::from("core.x-y"), value.clone())]),
                };
                let mut out = Out(String::new());
                out.put_quoted(&name).unwrap();
                prop_assert_eq!(out.0.as_str(), &*config::quote(&name));
                let file = encode(core::slice::from_ref(&profile)).unwrap();
                let read = decode(file.as_bytes()).unwrap().unwrap();
                prop_assert_eq!(read.len(), 1);
                // The same reading the standard accessor gives.
                let expected: Vec<_> = config::parse(&file)
                    .filter_map(Result::ok)
                    .filter(|e| e.section == "profile.1")
                    .map(|e| (String::from(e.key), String::from(&*e.value.as_str())))
                    .collect();
                let mut got = Vec::from([(String::from("name"), read[0].name.clone())]);
                got.extend(read[0].values.iter().map(|(k, v)| (k.clone(), v.clone())));
                prop_assert_eq!(got, expected);
            }
        }
    }

    #[test]
    fn the_file_code_survives_every_refused_allocation() {
        let profiles = [
            Profile {
                name: "Quote \" and \\ back\nnew".into(),
                values: Vec::from([("core.font-size".into(), " 18 ".into())]),
            },
            Profile {
                name: "Night".into(),
                values: Vec::from([("core.night-mode".into(), "on".into())]),
            },
        ];
        let file = encode(&profiles).unwrap();
        crate::fail_alloc::sweep(|| encode_within(&profiles, 4096).map(|t| t.len()));
        crate::fail_alloc::sweep(|| decode(file.as_bytes()).map(|p| p.map_or(0, |p| p.len())));
        let v1 = include_bytes!("../testdata/profiles.ini.v1");
        crate::fail_alloc::sweep(|| decode(v1).map(|p| p.map_or(0, |p| p.len())));
        crate::fail_alloc::sweep(|| clean("  Night   [mode] ").map(|n| n.map_or(0, |n| n.len())));
        crate::fail_alloc::sweep(|| Value::Int(i64::MIN).to_text().map(|t| t.len()));
        crate::fail_alloc::sweep(|| Value::parse(Kind::Choice, "core.x").map(|v| v.is_some()));
        crate::fail_alloc::sweep(|| profiles[0].try_clone().map(|p| p.values.len()));
    }
}
