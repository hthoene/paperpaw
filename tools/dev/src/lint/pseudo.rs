//! The pseudo-language: every English text of the app made longer and
//! written with accented letters between brackets, `[Šéţţîñĝš ~~~]`, so
//! that text too long for its place and text not translated through the
//! catalogs both show. It is a data plugin that adds a whole language
//! (`i18n/pseudo.ppcat` and `i18n/*.pseudo.ini`, language
//! [`super::PSEUDO`]), put into the library of sessions in that language.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::cli::Context;
use crate::session::Session;
use crate::variant::Variant;

/// The data plugin's id and folder name.
pub const PLUGIN: &str = "paperpaw-dev-pseudo";

/// The largest catalog file a data plugin may have is 64 KiB; files are
/// cut well below.
const FILE_BYTES: usize = 48 * 1024;

/// Letters as the pseudo-language writes them: all within Latin-1 and
/// Latin Extended-A, which every font of PaperPaw has.
const ACCENTED: [(char, char); 40] = [
    ('a', 'å'),
    ('c', 'ç'),
    ('d', 'ð'),
    ('e', 'é'),
    ('g', 'ĝ'),
    ('h', 'ĥ'),
    ('i', 'î'),
    ('j', 'ĵ'),
    ('k', 'ķ'),
    ('l', 'ļ'),
    ('n', 'ñ'),
    ('o', 'ö'),
    ('p', 'þ'),
    ('r', 'ŕ'),
    ('s', 'š'),
    ('t', 'ţ'),
    ('u', 'û'),
    ('w', 'ŵ'),
    ('y', 'ý'),
    ('z', 'ž'),
    ('A', 'Å'),
    ('C', 'Ç'),
    ('D', 'Ð'),
    ('E', 'É'),
    ('G', 'Ĝ'),
    ('H', 'Ĥ'),
    ('I', 'Î'),
    ('J', 'Ĵ'),
    ('K', 'Ķ'),
    ('L', 'Ļ'),
    ('N', 'Ñ'),
    ('O', 'Ö'),
    ('P', 'Þ'),
    ('R', 'Ŕ'),
    ('S', 'Š'),
    ('T', 'Ţ'),
    ('U', 'Û'),
    ('W', 'Ŵ'),
    ('Y', 'Ý'),
    ('Z', 'Ž'),
];

/// Whether `c` is a letter only the pseudo-language writes.
pub fn is_accented(c: char) -> bool {
    ACCENTED.iter().any(|&(_, a)| a == c)
}

fn accent(c: char) -> char {
    ACCENTED
        .iter()
        .find(|&&(plain, _)| plain == c)
        .map_or(c, |&(_, a)| a)
}

/// `text` in the pseudo-language: its letters accented, `{placeholders}`
/// kept as they are, between brackets and made longer with `~`: by about
/// a third, more for short texts (as translations of short texts grow
/// most), brackets included. Empty texts stay empty.
pub fn pseudo(text: &str) -> String {
    if text.trim().is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() * 2 + 8);
    out.push('[');
    let mut visible = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // `{{` is a brace, not a placeholder.
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push_str("{{");
                visible += 1;
            }
            '{' => {
                out.push('{');
                for c in chars.by_ref() {
                    out.push(c);
                    if c == '}' {
                        break;
                    }
                }
                // What a placeholder becomes is not known: a word.
                visible += 4;
            }
            c => {
                out.push(accent(c));
                visible += 1;
            }
        }
    }
    // Very short texts (abbreviations, the days in a chart) are only
    // accented: their translations are as short, and brackets would
    // double them.
    if visible <= 3 {
        out.remove(0);
        return out;
    }
    let grow = match visible {
        0..=5 => visible,
        6..=10 => visible * 7 / 10,
        11..=20 => visible / 2,
        _ => visible * 35 / 100,
    };
    // The brackets and the space before the filler count.
    let fill = grow.saturating_sub(3).max(1);
    out.push(' ');
    out.extend(std::iter::repeat_n('~', fill));
    out.push(']');
    out
}

/// The catalog files of the pseudo-language for `messages` (English, by
/// key). All of the app's texts made longer are more than a data plugin's
/// text catalogs may take together, so they are compiled into
/// `i18n/pseudo.ppcat` (ADR 0033), as in a language pack; the few longer
/// than a compiled catalog holds are in `i18n/<n>.pseudo.ini`, each below
/// the size a data plugin's file may have.
pub fn catalogs(messages: &BTreeMap<String, String>) -> Result<Files, String> {
    let header = format!(
        "[meta]\nlanguage = {}\nname = {}\nplural = one-other\n\n[messages]\n",
        super::PSEUDO,
        pseudo("Pseudo-language")
    );
    let mut files: Files = Vec::new();
    let mut compiled = header.clone();
    let mut body = String::new();
    let flush = |body: &mut String, files: &mut Files| {
        if !body.is_empty() {
            let name = format!("i18n/{:02}.pseudo.ini", files.len() + 1);
            files.push((name, format!("{header}{body}").into_bytes()));
            body.clear();
        }
    };
    for (key, value) in messages {
        let text = pseudo(value);
        let line = format!("{key} = {}\n", paperpaw_config::quote(&text));
        if text.len() <= paperpaw_i18n::MAX_MESSAGE {
            compiled.push_str(&line);
            continue;
        }
        if body.len() + line.len() + header.len() > FILE_BYTES {
            flush(&mut body, &mut files);
        }
        body.push_str(&line);
    }
    flush(&mut body, &mut files);
    let compiled = paperpaw_i18n::Catalog::compile(&compiled)
        .map_err(|e| format!("the pseudo-language's catalog: {e}"))?;
    files.push(("i18n/pseudo.ppcat".into(), compiled));
    Ok(files)
}

/// The plugin's `plugin.ini`.
fn manifest() -> String {
    format!(
        "[plugin]\nid = {PLUGIN}\nname = Pseudo-language\n\
         description = Every text longer and accented, for paperpaw-dev.\n\
         version = 0.1.0\nformats = 4\n"
    )
}

/// Every English message of the app (its plugins and the data plugins
/// looked at), by key, and the names and descriptions of the plugins
/// under the keys they are translated with.
pub fn english(session: &Session) -> BTreeMap<String, String> {
    let kernel = &session.device.kernel;
    let mut messages = BTreeMap::new();
    for plugin in kernel.plugins().iter() {
        let id = plugin.manifest.id.as_str();
        if id.starts_with(crate::probe::PLUGIN) {
            continue;
        }
        messages.insert(
            format!("{id}.plugin.name"),
            plugin.manifest.name.to_string(),
        );
        messages.insert(
            format!("{id}.plugin.description"),
            plugin.manifest.description.to_string(),
        );
    }
    // Later catalogs win, as in the translator.
    let english = super::languages(kernel)
        .into_iter()
        .find(|l| l.code == paperpaw_i18n::FALLBACK_LANGUAGE);
    if let Some(english) = english {
        messages.extend(english.messages);
    }
    messages
}

/// The files made for each set of data plugins and profiles, so that the
/// many sessions of a run make them once.
static MADE: Mutex<Vec<(Vec<String>, Files)>> = Mutex::new(Vec::new());

/// Files as `(path in the library, bytes)`.
pub type Files = Vec<(String, Vec<u8>)>;

/// The data plugin's files. Its texts are those of every profile looked
/// at (some plugins are there only on some devices, such as a front
/// light's).
pub fn plugin(context: &Context) -> Result<Files, String> {
    // A folder's place, or what is handed in by its content: changed and
    // checked again in the same run (the browser's), it is made anew.
    let mut key: Vec<String> = context
        .target
        .data
        .iter()
        .map(|d| match &d.folder {
            Some(_) => d.place(),
            None => format!("{}#{:016x}", d.id, d.digest()),
        })
        .collect();
    key.extend(context.profiles.iter().map(|p| p.device.id.to_string()));
    if let Ok(made) = MADE.lock()
        && let Some((_, files)) = made.iter().find(|(k, _)| *k == key)
    {
        return Ok(files.clone());
    }
    let mut variants = vec![Variant::x4()];
    for profile in &context.profiles {
        if variants
            .iter()
            .all(|v| v.profile.device.id != profile.device.id)
        {
            variants.push(Variant::of(profile));
        }
    }
    let mut messages = BTreeMap::new();
    for variant in &variants {
        let session = Session::new(context, variant)?;
        messages.extend(english(&session));
    }
    let folder = format!("plugins/{PLUGIN}");
    let mut files = vec![(format!("{folder}/plugin.ini"), manifest().into_bytes())];
    for (name, bytes) in catalogs(&messages)? {
        files.push((format!("{folder}/{name}"), bytes));
    }
    if let Ok(mut made) = MADE.lock() {
        made.push((key, files.clone()));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_grow_and_keep_placeholders() {
        assert_eq!(pseudo("Settings"), "[Šéţţîñĝš ~~]");
        assert_eq!(pseudo("We"), "Ŵé");
        let books = pseudo("{count} books of {author}");
        assert!(books.starts_with("[{count} bööķš öf {author} "), "{books}");
        assert!(books.ends_with("~]"));
        assert_eq!(pseudo("{{literal}}"), "[{{ļîţéŕåļ}} ~~~~]");
        assert_eq!(pseudo(""), "");
        assert_eq!(pseudo("  "), "  ");
        // About a third longer, at least.
        let long = "The time in words, the book you are reading and the ones before it.";
        let made = pseudo(long);
        assert!(
            made.chars().count() >= long.chars().count() * 13 / 10,
            "{made}"
        );
        assert!(made.chars().all(|c| (c as u32) < 0x180), "{made}");
    }

    /// A compiled catalog in memory.
    struct Bytes(Vec<u8>);

    impl paperpaw_i18n::CatalogFile for Bytes {
        fn read_exact_at(&self, offset: u32, buf: &mut [u8]) -> bool {
            let start = offset as usize;
            match self.0.get(start..start + buf.len()) {
                Some(bytes) => {
                    buf.copy_from_slice(bytes);
                    true
                }
                None => false,
            }
        }

        fn size(&self) -> u32 {
            u32::try_from(self.0.len()).unwrap()
        }
    }

    #[test]
    fn catalogs_are_compiled_the_longest_split_and_all_parse() {
        let mut messages = BTreeMap::new();
        for n in 0..3000 {
            messages.insert(format!("p.key-{n:04}"), format!("Message number {n}"));
        }
        // Longer made pseudo than a compiled catalog holds, more of them
        // than one text file may take.
        let long = "A text of help that goes on. ".repeat(80);
        for n in 0..40 {
            messages.insert(format!("p.how-{n:02}"), long.clone());
        }
        messages.insert("p.lines".into(), "Two\nlines".into());
        messages.insert("p.items.one".into(), "{count} item".into());
        messages.insert("p.items.other".into(), "{count} items".into());
        let files = catalogs(&messages).unwrap();
        let mut keys = 0;
        let mut texts = 0;
        for (name, bytes) in &files {
            let catalog = if name == "i18n/pseudo.ppcat" {
                paperpaw_i18n::Catalog::open(std::rc::Rc::new(Bytes(bytes.clone()))).unwrap()
            } else {
                assert!(name.ends_with(".pseudo.ini"), "{name}");
                assert!(bytes.len() < 64 * 1024);
                texts += 1;
                let text = String::from_utf8(bytes.clone()).unwrap();
                paperpaw_i18n::Catalog::parse(text).unwrap()
            };
            assert_eq!(catalog.language(), super::super::PSEUDO);
            keys += catalog.len();
            if let Some(lines) = catalog.get_copy("p.lines") {
                assert!(lines.starts_with("[Ţŵö\nļîñéš"), "{lines}");
            }
            if let Some(one) = catalog.get_copy("p.items.one") {
                assert!(one.starts_with("[{count} îţém"), "{one}");
            }
        }
        assert!(files.iter().any(|(name, _)| name == "i18n/pseudo.ppcat"));
        assert!(texts > 1, "the longest are split: {texts}");
        assert_eq!(keys, messages.len());
    }
}
