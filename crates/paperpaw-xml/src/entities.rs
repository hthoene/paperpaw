use alloc::borrow::Cow;
use alloc::collections::TryReserveError;
use alloc::string::String;

/// Named entities common in e-books (XML's five plus frequent HTML ones).
const NAMED: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("shy", '\u{ad}'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("thinsp", '\u{2009}'),
    ("zwnj", '\u{200c}'),
    ("zwj", '\u{200d}'),
    ("ndash", '\u{2013}'),
    ("mdash", '\u{2014}'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("sbquo", '‚'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("bdquo", '„'),
    ("laquo", '«'),
    ("raquo", '»'),
    ("lsaquo", '‹'),
    ("rsaquo", '›'),
    ("hellip", '…'),
    ("bull", '•'),
    ("middot", '·'),
    ("dagger", '†'),
    ("Dagger", '‡'),
    ("prime", '′'),
    ("Prime", '″'),
    ("copy", '©'),
    ("reg", '®'),
    ("trade", '™'),
    ("deg", '°'),
    ("times", '×'),
    ("divide", '÷'),
    ("plusmn", '±'),
    ("frac12", '½'),
    ("frac14", '¼'),
    ("frac34", '¾'),
    ("sect", '§'),
    ("para", '¶'),
    ("euro", '€'),
    ("pound", '£'),
    ("yen", '¥'),
    ("cent", '¢'),
    ("iexcl", '¡'),
    ("iquest", '¿'),
    ("szlig", 'ß'),
    ("auml", 'ä'),
    ("ouml", 'ö'),
    ("uuml", 'ü'),
    ("Auml", 'Ä'),
    ("Ouml", 'Ö'),
    ("Uuml", 'Ü'),
    ("aacute", 'á'),
    ("eacute", 'é'),
    ("iacute", 'í'),
    ("oacute", 'ó'),
    ("uacute", 'ú'),
    ("agrave", 'à'),
    ("egrave", 'è'),
    ("igrave", 'ì'),
    ("ograve", 'ò'),
    ("ugrave", 'ù'),
    ("acirc", 'â'),
    ("ecirc", 'ê'),
    ("icirc", 'î'),
    ("ocirc", 'ô'),
    ("ucirc", 'û'),
    ("ccedil", 'ç'),
    ("ntilde", 'ñ'),
    ("aring", 'å'),
    ("aelig", 'æ'),
    ("oslash", 'ø'),
    ("Eacute", 'É'),
    ("Ccedil", 'Ç'),
];

paperpaw_table::shared! {
/// Decodes character and entity references; unknown or malformed ones are
/// kept as written.
pub fn decode(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    decode_into(text, &mut out);
    Cow::Owned(out)
} @ data::XML_DECODE
}

/// As [`decode`], with the memory error instead of an abort: the decoded
/// text takes one fallible reservation of at most `text`'s length (a
/// reference is never shorter than what it stands for), and text without
/// `&` is borrowed as it is. On the error nothing is kept.
///
/// # Errors
/// [`TryReserveError`] where the heap cannot give the reservation.
pub fn try_decode(text: &str) -> Result<Cow<'_, str>, TryReserveError> {
    if !text.contains('&') {
        return Ok(Cow::Borrowed(text));
    }
    let mut out = String::new();
    out.try_reserve_exact(text.len())?;
    decode_into(text, &mut out);
    Ok(Cow::Owned(out))
}

/// Writes `text` decoded onto `out`, which has room for `text.len()` more
/// bytes: the decoded text is never longer, so `out` does not grow.
fn decode_into(text: &str, out: &mut String) {
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        // Only the next few bytes can end a reference: a text full of `&`
        // is not searched to its end for each of them.
        let decoded = tail[1..]
            .bytes()
            .take(33)
            .position(|b| b == b';')
            .and_then(|end| resolve(&tail[1..1 + end]).map(|c| (c, end + 2)));
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
}

fn resolve(entity: &str) -> Option<char> {
    if let Some(number) = entity.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|&c| c != '\0');
    }
    NAMED
        .iter()
        .find(|(name, _)| *name == entity)
        .map(|(_, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_named_and_numeric_references() {
        assert_eq!(decode("a &amp; b"), "a & b");
        assert_eq!(
            decode("&#8220;Hi&#x201D; &mdash; &auml;"),
            "“Hi” \u{2014} ä"
        );
        assert_eq!(decode("&unknown; & &#xZZ; &;"), "&unknown; & &#xZZ; &;");
        assert!(matches!(decode("plain"), Cow::Borrowed(_)));
    }

    /// The decoded text is never longer than the text: every reference
    /// is at least as long as the character it stands for, so the one
    /// reservation of `try_decode` (and `decode`'s capacity) is never grown.
    #[test]
    fn a_reference_is_never_shorter_than_its_character() {
        for (name, c) in NAMED {
            assert!(name.len() + 2 >= c.len_utf8(), "&{name};");
        }
        for code in [0x7f_u32, 0x80, 0x7ff, 0x800, 0xffff, 0x1_0000, 0x10_ffff] {
            let c = char::from_u32(code).unwrap();
            let decimal = alloc::format!("&#{code};");
            let hex = alloc::format!("&#x{code:x};");
            assert!(decimal.len() >= c.len_utf8() && hex.len() >= c.len_utf8());
        }
        let text = "&amp;&lt;&#x10FFFF;&#1;&mdash;&auml; &unknown; &";
        let decoded = try_decode(text).unwrap();
        assert!(decoded.len() <= text.len());
        assert_eq!(decoded, decode(text));
        assert!(matches!(try_decode("plain"), Ok(Cow::Borrowed(_))));
    }
}
