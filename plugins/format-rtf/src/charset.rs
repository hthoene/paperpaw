//! The code pages of the bytes of an RTF file (`\'hh`, or any byte above
//! 0x7f in the text): Windows-1252 (the default), Windows-1250 and Windows-1251.
//! Any other page reads as Windows-1252; the files that need one of the
//! multi-byte pages (Japanese, Chinese, Korean) write every character as
//! `\uN` besides, and those are read exactly.

const CP1250: [char; 128] = [
    '€', '\u{fffd}', '‚', '\u{fffd}', '„', '…', '†', '‡', '\u{fffd}', '‰', 'Š', '‹', 'Ś', 'Ť', 'Ž',
    'Ź', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '\u{fffd}', '™', 'š', '›', 'ś', 'ť', 'ž',
    'ź', '\u{a0}', 'ˇ', '˘', 'Ł', '¤', 'Ą', '¦', '§', '¨', '©', 'Ş', '«', '¬', '\u{ad}', '®', 'Ż',
    '°', '±', '˛', 'ł', '´', 'µ', '¶', '·', '¸', 'ą', 'ş', '»', 'Ľ', '˝', 'ľ', 'ż', 'Ŕ', 'Á', 'Â',
    'Ă', 'Ä', 'Ĺ', 'Ć', 'Ç', 'Č', 'É', 'Ę', 'Ë', 'Ě', 'Í', 'Î', 'Ď', 'Đ', 'Ń', 'Ň', 'Ó', 'Ô', 'Ő',
    'Ö', '×', 'Ř', 'Ů', 'Ú', 'Ű', 'Ü', 'Ý', 'Ţ', 'ß', 'ŕ', 'á', 'â', 'ă', 'ä', 'ĺ', 'ć', 'ç', 'č',
    'é', 'ę', 'ë', 'ě', 'í', 'î', 'ď', 'đ', 'ń', 'ň', 'ó', 'ô', 'ő', 'ö', '÷', 'ř', 'ů', 'ú', 'ű',
    'ü', 'ý', 'ţ', '˙',
];

const CP1251: [char; 128] = [
    'Ђ', 'Ѓ', '‚', 'ѓ', '„', '…', '†', '‡', '€', '‰', 'Љ', '‹', 'Њ', 'Ќ', 'Ћ', 'Џ', 'ђ', '‘', '’',
    '“', '”', '•', '–', '—', '\u{fffd}', '™', 'љ', '›', 'њ', 'ќ', 'ћ', 'џ', '\u{a0}', 'Ў', 'ў',
    'Ј', '¤', 'Ґ', '¦', '§', 'Ё', '©', 'Є', '«', '¬', '\u{ad}', '®', 'Ї', '°', '±', 'І', 'і', 'ґ',
    'µ', '¶', '·', 'ё', '№', 'є', '»', 'ј', 'Ѕ', 'ѕ', 'ї', 'А', 'Б', 'В', 'Г', 'Д', 'Е', 'Ж', 'З',
    'И', 'Й', 'К', 'Л', 'М', 'Н', 'О', 'П', 'Р', 'С', 'Т', 'У', 'Ф', 'Х', 'Ц', 'Ч', 'Ш', 'Щ', 'Ъ',
    'Ы', 'Ь', 'Э', 'Ю', 'Я', 'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н',
    'о', 'п', 'р', 'с', 'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ', 'ъ', 'ы', 'ь', 'э', 'ю', 'я',
];

/// The characters 0x80 to 0x9f of Windows-1252 (the rest is Latin-1).
const CP1252: [char; 32] = [
    '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}', 'Ž',
    '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{fffd}',
    'ž', 'Ÿ',
];

/// The code page of the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Charset {
    Windows1250,
    Windows1251,
    Windows1252,
}

impl Charset {
    /// The page `\ansicpgN` names.
    pub(crate) const fn of_page(page: u32) -> Self {
        match page {
            1250 => Self::Windows1250,
            1251 => Self::Windows1251,
            _ => Self::Windows1252,
        }
    }

    /// The character of `byte`.
    pub(crate) fn decode(self, byte: u8) -> char {
        if byte < 0x80 {
            return char::from(byte);
        }
        let at = usize::from(byte - 0x80);
        match self {
            Self::Windows1250 => CP1250[at],
            Self::Windows1251 => CP1251[at],
            Self::Windows1252 if byte < 0xa0 => CP1252[at],
            Self::Windows1252 => char::from(byte),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages() {
        assert_eq!(Charset::of_page(1252).decode(0xe9), 'é');
        assert_eq!(Charset::of_page(1252).decode(0x93), '“');
        assert_eq!(Charset::of_page(1250).decode(0xe8), 'č');
        assert_eq!(Charset::of_page(1251).decode(0xcf), 'П');
        assert_eq!(Charset::of_page(932).decode(0x41), 'A');
        assert_eq!(Charset::of_page(0).decode(0x80), '€');
    }
}
