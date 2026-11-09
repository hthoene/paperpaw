//! Reading one string member of a JSON object without a limit on its length
//! and without an abort when memory runs short: the member is found by
//! scanning the object (names are compared as they are decoded, nothing is
//! allocated) and its text is unescaped into a fallible buffer. The caller
//! bounds the object's size first.
use alloc::string::String;
use paperpaw_sdk::native::Error;

/// The string member `key` of `object` (text that starts with `{`), as the
/// interface-6 reader gives it: `None` where there is no such member or it
/// is not a string, the first of a repeated key.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn string_member(object: &str, key: &str) -> Result<Option<String>, Error> {
    match raw_member(object, key) {
        Some(literal) => unquote(literal),
        None => Ok(None),
    }
}

/// The string a JSON string literal (with its quotes) stands for; `None`
/// where it is not one or is cut short. A lone surrogate stands for U+FFFD.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn unquote(literal: &str) -> Result<Option<String>, Error> {
    // Count first, so that one exact reservation holds the text: a doubling
    // buffer would take twice the memory of a long summary.
    let mut bytes = 0usize;
    let ok = decode(literal, &mut |c| {
        bytes += c.len_utf8();
        Ok(())
    })?;
    if !ok {
        return Ok(None);
    }
    let mut out = String::new();
    out.try_reserve_exact(bytes).map_err(|_| Error::NoMemory)?;
    decode(literal, &mut |c| {
        out.push(c);
        Ok(())
    })?;
    Ok(Some(out))
}

/// The JSON text of member `key` of `text`, as it is written; `None` where
/// the object is malformed before the member or has none. Allocates nothing.
fn raw_member<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let bytes = text.as_bytes();
    let mut i = skip_space(bytes, 0);
    if bytes.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    loop {
        i = skip_space(bytes, i);
        if bytes.get(i) != Some(&b'"') {
            return None;
        }
        let name_end = string_end(bytes, i)?;
        let mut wanted = key.chars();
        let mut same = true;
        let ok = decode(&text[i..name_end], &mut |c| {
            same = same && wanted.next() == Some(c);
            Ok(())
        })
        .ok()?;
        if !ok {
            return None;
        }
        let same = same && wanted.next().is_none();
        i = skip_space(bytes, name_end);
        if bytes.get(i) != Some(&b':') {
            return None;
        }
        let start = skip_space(bytes, i + 1);
        let end = value_end(bytes, start)?;
        if same {
            return Some(text[start..end].trim_end());
        }
        i = skip_space(bytes, end);
        match bytes.get(i) {
            Some(b',') => i += 1,
            _ => return None,
        }
    }
}

/// Gives each character of a string literal to `sink`; false where it is
/// not one or is cut short.
fn decode(literal: &str, sink: &mut dyn FnMut(char) -> Result<(), Error>) -> Result<bool, Error> {
    let mut chars = literal.trim().chars();
    if chars.next() != Some('"') {
        return Ok(false);
    }
    // A high surrogate waiting for its low half.
    let mut high: Option<u32> = None;
    loop {
        let Some(next) = chars.next() else {
            return Ok(false);
        };
        let c = match next {
            '"' => {
                if high.is_some() {
                    sink('\u{fffd}')?;
                }
                return Ok(true);
            }
            '\\' => match chars.next() {
                None => return Ok(false),
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some('b') => '\u{8}',
                Some('f') => '\u{c}',
                Some('u') => {
                    let Some(unit) = hex4(&mut chars) else {
                        return Ok(false);
                    };
                    let waiting = high.take();
                    if let Some(high) = waiting
                        && (0xdc00..0xe000).contains(&unit)
                    {
                        let code = 0x10000 + ((high - 0xd800) << 10) + (unit - 0xdc00);
                        sink(char::from_u32(code).unwrap_or('\u{fffd}'))?;
                        continue;
                    }
                    if waiting.is_some() {
                        sink('\u{fffd}')?;
                    }
                    if (0xd800..0xdc00).contains(&unit) {
                        high = Some(unit);
                    } else {
                        sink(char::from_u32(unit).unwrap_or('\u{fffd}'))?;
                    }
                    continue;
                }
                Some(c) => c,
            },
            c => c,
        };
        if high.take().is_some() {
            sink('\u{fffd}')?;
        }
        sink(c)?;
    }
}

fn hex4(chars: &mut core::str::Chars<'_>) -> Option<u32> {
    let mut value = 0;
    for _ in 0..4 {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    Some(value)
}

fn skip_space(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// Where the string whose `"` is at `i` ends, after its closing `"`.
fn string_end(bytes: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// Where the value starting at `i` ends.
fn value_end(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes.get(i)? {
        b'"' => string_end(bytes, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < bytes.len() {
                match bytes[j] {
                    b'"' => {
                        j = string_end(bytes, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while j < bytes.len() && !matches!(bytes[j], b',' | b'}' | b']') {
                j += 1;
            }
            Some(j)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec::Vec};

    /// Literals of every kind of escape and of every way to be wrong.
    fn literals() -> Vec<String> {
        let pieces = [
            "a",
            " ",
            "é",
            "日",
            "😀",
            "\\n",
            "\\t",
            "\\r",
            "\\b",
            "\\f",
            "\\\"",
            "\\\\",
            "\\/",
            "\\x",
            "\\u00e4",
            "\\u0041",
            "\\ud83d\\ude00",
            "\\ud83d",
            "\\ude00",
            "\\ud83dx",
            "\\ud83d\\u0041",
            "\\uZZZZ",
            "\\u12",
            "\\",
            "\"",
        ];
        let mut out = Vec::new();
        for a in pieces {
            out.push(format!("\"{a}\""));
            out.push(format!("\"{a}"));
            for b in pieces {
                out.push(format!("\"{a}{b}\""));
                for c in ["x", "\\ud83d", "\\ude00"] {
                    out.push(format!("\"{a}{b}{c}\""));
                }
            }
        }
        for plain in ["", "null", "42", "true", "[1]", "{}", "  \"padded\"  "] {
            out.push(plain.into());
        }
        out
    }

    /// A summary as the encyclopedia answers: nested namespace, titles and
    /// thumbnail before the extract, escapes and non-ASCII in the text.
    const WIKIPEDIA: &str = r#"{"type":"standard","title":"Ada Lovelace","displaytitle":"<span class=\"mw-page-title-main\">Ada Lovelace</span>","namespace":{"id":0,"text":""},"wikibase_item":"Q7259","titles":{"canonical":"Ada_Lovelace","normalized":"Ada Lovelace","display":"Ada Lovelace"},"pageid":1,"thumbnail":{"source":"https://upload.example/thumb/a.jpg","width":320,"height":400},"lang":"en","dir":"ltr","description":"English mathematician (1815\u20131852)","extract":"Augusta Ada King, Countess of Lovelace (n\u00e9e Byron) was an English mathematician.\nShe wrote \"Notes\" \ud83d\udcda.","extract_html":"<p>Augusta</p>"}"#;

    #[test]
    fn the_extract_follows_the_nested_members() {
        let extract = string_member(WIKIPEDIA, "extract").unwrap().unwrap();
        assert!(extract.starts_with("Augusta Ada King"));
        assert!(extract.ends_with("Notes\" \u{1f4da}."));
        assert_eq!(string_member(WIKIPEDIA, "text").unwrap(), None);
    }

    #[test]
    fn unquote_is_the_interface_6_unquote() {
        for literal in literals() {
            assert_eq!(
                unquote(&literal).unwrap(),
                paperpaw_sdk::json::unquote(&literal),
                "{literal}"
            );
        }
    }

    #[test]
    fn a_member_is_found_as_the_interface_6_reader_finds_it() {
        let objects = [
            r#"{"a":1,"text":"Hallo \"Welt\"","b":2}"#,
            r#"{"text":"x","text":"y"}"#,
            r#"{"nested":{"text":"inner"},"text":"outer"}"#,
            r#"{"text":null}"#,
            r#"{"text":5}"#,
            r#"{"other":"x"}"#,
            WIKIPEDIA,
            r#"{"te\u0078t":"escaped name"}"#,
            r#"{"tex":"short","texts":"long","text":"exact"}"#,
            r#"{"bad\uZZ":"x","text":"after a bad name"}"#,
            r#"{"a":[1,{"text":"deep"}],"text":"after nested"}"#,
            r#"  {  "text"  :  "spaced"  }"#,
            "{}",
            "",
            "not json",
        ];
        for object in objects {
            let members = paperpaw_sdk::json::object(object);
            let want = paperpaw_sdk::json::text(&members, "text").map(String::from);
            assert_eq!(string_member(object, "text").unwrap(), want, "{object}");
        }
    }

    #[test]
    fn a_long_text_is_kept_whole() {
        let long = "ä".repeat(6000);
        let object = format!(r#"{{"text":"{long}"}}"#);
        assert_eq!(string_member(&object, "text").unwrap(), Some(long));
    }

    #[test]
    fn refused_allocations_end_in_no_memory() {
        crate::fail_alloc::sweep(|| {
            string_member(r#"{"a":"b","text":"Hallo ä 😀 \"x\""}"#, "text").map(|t| t.is_some())
        });
    }
}
