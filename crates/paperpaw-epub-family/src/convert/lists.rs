//! List items: their markers (`list-style-type`, the `type` and `value`
//! attributes, numbers as letters or Roman numerals) and the hanging
//! indent that sets an item's lines after its marker.
//!
//! A marker waits for the item's first text, so an item that starts with
//! a paragraph (`<li><p>…`) has its marker on that paragraph's line, not
//! on a line of its own.

use alloc::string::String;

use paperpaw_sdk::document::{DocumentError, TextStyle};
use paperpaw_sdk::xml::Attrs;

use super::{Converter, Frame};
use crate::css::{ListStyle, Props};
use crate::fallible::{self, TryPush};

/// Most of a marker's width the lines of an item hang by, in hundredths
/// of an em: a longer marker (a huge number) pushes its first line on.
const MAX_HANG: u16 = 400;

/// Room between a marker and its item's text: a no-break space, so a
/// justified line does not stretch it and the text stays after the
/// marker.
const GAP: char = '\u{a0}';

/// A list item's marker, waiting for the item's first text.
pub(super) struct Marker {
    text: String,
    style: TextStyle,
    /// The marker's width, in hundredths of an em: the item's lines are
    /// set in by it and its first line starts back by it.
    hang: u16,
}

/// The marker style of an element: a list's own (`<ol>` numbers, `<ul>`
/// bullets, as browsers' own stylesheet sets them), else inherited.
pub(super) fn list_style(tag: &str, props: &Props, parent: Option<&Frame>) -> Option<ListStyle> {
    match tag {
        "ol" => Some(props.list_style.unwrap_or(ListStyle::Decimal)),
        "ul" => Some(props.list_style.unwrap_or(ListStyle::Disc)),
        _ => props
            .list_style
            .or_else(|| parent.and_then(|f| f.list_style)),
    }
}

/// The items counted so far of a list, `<ol start>` less one.
pub(super) fn counter(tag: &str, attrs: &Attrs<'_>) -> Result<Option<u32>, DocumentError> {
    Ok(match tag {
        "ol" => Some(number(attrs, "start")?.map_or(0, |s| s.saturating_sub(1))),
        "ul" => Some(0),
        _ => None,
    })
}

/// A list's or an item's `type` attribute: `1 a A i I` or a bullet's
/// name. Case matters for letters and numerals, as in HTML.
pub(super) fn type_attribute(value: &str) -> Option<ListStyle> {
    match value.trim() {
        "1" => Some(ListStyle::Decimal),
        "a" => Some(ListStyle::LowerAlpha),
        "A" => Some(ListStyle::UpperAlpha),
        "i" => Some(ListStyle::LowerRoman),
        "I" => Some(ListStyle::UpperRoman),
        value => ListStyle::parse_nocase(value),
    }
}

fn number(attrs: &Attrs<'_>, name: &str) -> Result<Option<u32>, DocumentError> {
    let value = attrs.try_get(name).map_err(|_| fallible::OOM)?;
    Ok(value.and_then(|s| s.trim().parse::<u32>().ok()))
}

/// `number` as `style` writes it, with its punctuation; `None` for no
/// marker.
fn marker_text(style: ListStyle, number: u32) -> Result<Option<String>, DocumentError> {
    // The longest marker is `ZZZZZZZ.` and its gap, or ten digits and a point.
    let mut text = fallible::string_with(40)?;
    match style {
        ListStyle::None => return Ok(None),
        ListStyle::Disc => text.try_push('•')?,
        ListStyle::Circle => text.try_push('◦')?,
        ListStyle::Square => text.try_push('▪')?,
        ListStyle::LowerAlpha | ListStyle::UpperAlpha if number > 0 => {
            alpha(&mut text, number, style == ListStyle::UpperAlpha)?;
            text.try_push('.')?;
        }
        ListStyle::LowerRoman | ListStyle::UpperRoman if (1..4000).contains(&number) => {
            roman(&mut text, number, style == ListStyle::UpperRoman)?;
            text.try_push('.')?;
        }
        // Decimal, and numbers the others cannot write (0, 4000 and on).
        _ => fallible::try_write!(&mut text, "{number}.")?,
    }
    text.try_push(GAP)?;
    Ok(Some(text))
}

/// a … z, aa … az, ba …: bijective base 26, as CSS counts.
fn alpha(out: &mut String, mut number: u32, upper: bool) -> Result<(), DocumentError> {
    let mut letters = [0u8; 7];
    let mut at = letters.len();
    while number > 0 && at > 0 {
        number -= 1;
        at -= 1;
        letters[at] = if upper { b'A' } else { b'a' } + (number % 26) as u8;
        number /= 26;
    }
    for &b in &letters[at..] {
        out.try_push(char::from(b))?;
    }
    Ok(())
}

/// 1 to 3999 in Roman numerals.
fn roman(out: &mut String, mut number: u32, upper: bool) -> Result<(), DocumentError> {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    for (value, numeral) in NUMERALS {
        while number >= value {
            for c in numeral.chars() {
                out.try_push(if upper { c.to_ascii_uppercase() } else { c })?;
            }
            number -= value;
        }
    }
    Ok(())
}

/// About how wide `text` is in a book's font, in hundredths of an em:
/// the widths of a common serif face. The reader's font differs a little;
/// the text after the marker then stands a little before or after the
/// lines below it.
fn width(text: &str) -> u16 {
    let width: u32 = text
        .chars()
        .map(|c| match c {
            '0'..='9' | 'v' | 'x' | 'd' => 50,
            '.' | GAP => 25,
            'i' | 'l' | 'j' => 28,
            'I' => 33,
            'm' | 'w' => 78,
            'M' | 'W' => 89,
            'a'..='z' => 47,
            'A'..='Z' => 70,
            _ => 45,
        })
        .sum();
    width.min(u32::from(MAX_HANG)) as u16
}

impl Converter<'_> {
    /// An `<li>` starts: counted in the nearest list, its marker made to
    /// wait for its text and the item's lines set in by the marker.
    pub(super) fn list_item(&mut self, attrs: &Attrs<'_>) -> Result<(), DocumentError> {
        // An item before whose marker no text came (a list in it): the
        // marker on its own line, set in as that item.
        if self.marker.is_some() {
            let outer = self.stack.iter().rev().skip(1).find(|f| f.tag == "li");
            let margins = outer.map(|f| f.margins);
            self.place_marker()?;
            self.margins = margins.unwrap_or(self.margins);
            self.flush()?;
        }
        let value = number(attrs, "value")?;
        let list = self.stack.iter_mut().rev().find(|f| f.numbered.is_some());
        let number = match list.and_then(|f| f.numbered.as_mut()) {
            Some(count) => {
                *count = value.unwrap_or_else(|| count.saturating_add(1));
                *count
            }
            None => value.unwrap_or(1),
        };
        let Some(item) = self.stack.last_mut() else {
            return Ok(());
        };
        let Some(text) = marker_text(item.list_style.unwrap_or(ListStyle::Disc), number)? else {
            return Ok(());
        };
        let hang = width(&text);
        item.margins.0 = item.margins.0.saturating_add(hang);
        let style = item.style;
        self.marker = Some(Marker { text, style, hang });
        Ok(())
    }

    /// Writes the waiting marker before the first text of a block, which
    /// then starts back by the marker's width.
    pub(super) fn place_marker(&mut self) -> Result<(), DocumentError> {
        let Some(marker) = self.marker.take() else {
            return Ok(());
        };
        // The text's place in its node is the text's, not the marker's.
        let origin = self.origin.take();
        for c in marker.text.chars() {
            self.push_styled(c, marker.style)?;
        }
        self.origin = origin;
        self.indent = Some(-i16::try_from(marker.hang).unwrap_or(0));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(style: ListStyle, number: u32) -> String {
        marker_text(style, number).unwrap().unwrap_or_default()
    }

    #[test]
    fn letters_count_as_css_counts() {
        let lower = |n| marker(ListStyle::LowerAlpha, n);
        assert_eq!(lower(1), "a.\u{a0}");
        assert_eq!(lower(26), "z.\u{a0}");
        assert_eq!(lower(27), "aa.\u{a0}");
        assert_eq!(lower(702), "zz.\u{a0}");
        assert_eq!(lower(703), "aaa.\u{a0}");
        assert_eq!(marker(ListStyle::UpperAlpha, 28), "AB.\u{a0}");
        // No letter for 0: a number.
        assert_eq!(lower(0), "0.\u{a0}");
    }

    #[test]
    fn roman_numerals_up_to_3999() {
        let upper = |n| marker(ListStyle::UpperRoman, n);
        assert_eq!(upper(4), "IV.\u{a0}");
        assert_eq!(upper(1994), "MCMXCIV.\u{a0}");
        assert_eq!(upper(3999), "MMMCMXCIX.\u{a0}");
        assert_eq!(upper(4000), "4000.\u{a0}");
        assert_eq!(marker(ListStyle::LowerRoman, 49), "xlix.\u{a0}");
        assert_eq!(marker_text(ListStyle::None, 1), Ok(None));
    }

    #[test]
    fn hanging_grows_with_the_marker_up_to_a_limit() {
        assert!(width("•\u{a0}") < width("1.\u{a0}"));
        assert!(width("1.\u{a0}") < width("10.\u{a0}"));
        assert_eq!(width(&"m".repeat(50)), MAX_HANG);
    }
}
