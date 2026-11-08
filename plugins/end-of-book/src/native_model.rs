//! What the screen shows and what a press does, with no host handle, so it
//! can be tested without a device.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    Item,
    mem::push,
    text::{concat, copy, decimal_u64},
};

/// The most stars.
pub(crate) const STARS: u8 = 5;

/// The three rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    Next,
    Rate,
    Finished,
}

/// The next book of the series, as the screen offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Next {
    pub path: String,
    pub title: String,
    /// Its number in the series, as the book says (may be empty).
    pub number: String,
    /// The books the series has.
    pub total: u32,
}

/// The words of the three rows, from the catalogue.
#[derive(Debug, Default)]
pub(crate) struct Texts {
    pub next_title: String,
    pub next_note: String,
    pub rate_title: String,
    pub rate_value: String,
    pub rate_note: String,
    pub finished_title: String,
    pub finished_value: String,
    pub finished_note: String,
}

/// The rating after a press: one to five, then not rated again.
pub(crate) const fn next_stars(stars: u8) -> u8 {
    (stars + 1) % (STARS + 1)
}

#[allow(
    dead_code,
    reason = "reading the field back waits for the SDK's LIB op 22 (PORTING-7.md)"
)]
/// The rating a book field holds, 0 for none or anything but "1" to "5".
pub(crate) fn parse_stars(value: &str) -> u8 {
    match value.as_bytes() {
        [digit @ b'1'..=b'5'] => digit - b'0',
        _ => 0,
    }
}

/// The value the field is written with: "" removes the rating.
pub(crate) fn field_value(stars: u8) -> Result<String, Error> {
    if stars == 0 {
        return Ok(String::new());
    }
    let mut digits = [0; 20];
    copy(decimal_u64(u64::from(stars), &mut digits))
}

/// "4 / 5" for a rated book; `none` where there is no rating.
pub(crate) fn stars_text(stars: u8, none: &str) -> Result<String, Error> {
    if stars == 0 {
        return copy(none);
    }
    let mut digits = [0; 20];
    let mut limit = [0; 20];
    concat(&[
        decimal_u64(u64::from(stars), &mut digits),
        " / ",
        decimal_u64(u64::from(STARS), &mut limit),
    ])
}

/// The rows of the screen.
pub(crate) fn rows(texts: &Texts) -> Result<Vec<(Row, Item)>, Error> {
    let mut rows = Vec::new();
    push(
        &mut rows,
        (
            Row::Next,
            Item::noted(&texts.next_title, "\u{203a}", &texts.next_note)?,
        ),
    )?;
    push(
        &mut rows,
        (
            Row::Rate,
            Item::noted(&texts.rate_title, &texts.rate_value, &texts.rate_note)?,
        ),
    )?;
    push(
        &mut rows,
        (
            Row::Finished,
            Item::noted(
                &texts.finished_title,
                &texts.finished_value,
                &texts.finished_note,
            )?,
        ),
    )?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn a_rating_goes_from_one_to_five_and_is_then_not_rated_again() {
        let mut stars = 0;
        let mut seen = std::vec::Vec::new();
        for _ in 0..7 {
            stars = next_stars(stars);
            seen.push(stars);
        }
        assert_eq!(seen, [1, 2, 3, 4, 5, 0, 1]);
    }

    #[test]
    fn the_field_holds_one_digit_and_anything_else_is_not_a_rating() {
        for stars in 0..=STARS {
            assert_eq!(parse_stars(&field_value(stars).unwrap()), stars);
        }
        for value in ["", "0", "6", "10", "x", "٣", "4 "] {
            assert_eq!(parse_stars(value), 0, "{value:?}");
        }
        assert_eq!(field_value(0).unwrap(), "");
    }

    #[test]
    fn stars_read_four_of_five_and_none_says_so() {
        assert_eq!(stars_text(4, "Not rated").unwrap(), "4 / 5");
        assert_eq!(stars_text(0, "Not rated").unwrap(), "Not rated");
    }

    #[test]
    fn the_rows_are_next_rate_finished() {
        let rows = rows(&Texts::default()).unwrap();
        let kinds: Vec<Row> = rows.iter().map(|(row, _)| *row).collect();
        assert_eq!(kinds, [Row::Next, Row::Rate, Row::Finished]);
    }
}
