//! A [`ToneCurve`] as text: how a theme or a setting says what it wants.
//!
//! One curve is `smooth`, `contrast:-1` to `contrast:2`, `sharp:1` to
//! `sharp:255` or 16 numbers from 0 to 255, separated by spaces or
//! commas. A setting's value is a list of entries, `[script=]curve`,
//! separated by `;`: `smooth; arab=contrast:1; hani=0 20 40 ...`.

use alloc::string::String;

use crate::ToneCurve;

impl ToneCurve {
    /// ONE curve: `smooth`, `contrast:-1` to `contrast:2`, `sharp:1` to
    /// `sharp:255` (the smooth curve without grays), or 16 numbers from 0
    /// to 255 separated by spaces or commas. `None` for anything else,
    /// never a panic: the caller keeps the stock curve.
    pub fn parse(text: &str) -> Option<ToneCurve> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("smooth") {
            return Some(ToneCurve::SMOOTH);
        }
        if let Some((name, number)) = text.split_once(':') {
            let name = name.trim();
            let number = number.trim();
            if name.eq_ignore_ascii_case("contrast") {
                let step: i8 = number.parse().ok()?;
                return (ToneCurve::LIGHTEST..=ToneCurve::DARKEST)
                    .contains(&step)
                    .then(|| ToneCurve::contrast(step));
            }
            if name.eq_ignore_ascii_case("sharp") {
                let threshold: u8 = number.parse().ok()?;
                return (threshold > 0).then(|| ToneCurve::SMOOTH.sharp(threshold));
            }
            return None;
        }
        let mut alphas = [0u8; 16];
        let mut count = 0;
        for number in text.split([' ', ',', '\t']).filter(|n| !n.is_empty()) {
            *alphas.get_mut(count)? = number.parse().ok()?;
            count += 1;
        }
        (count == 16).then(|| ToneCurve::from_alphas(alphas))
    }

    /// A setting's whole value: a list of entries `[script=]curve`
    /// separated by `;`. Entries are tried in order: the first scripted
    /// entry whose key equals `script` (an ISO 15924 name such as `Arab`,
    /// compared without case) and that parses wins; failing that, the
    /// first unscripted entry that parses; else `None` (the caller keeps
    /// the stock curve). With no `script` only unscripted entries apply.
    ///
    /// A `key=` entry whose key is not four ASCII letters is ignored, as
    /// are empty entries and the spaces around `;` and `=`: a later form
    /// of entry is skipped by this reader, never read as a curve. Never a
    /// panic.
    pub fn parse_for(text: &str, script: Option<&str>) -> Option<ToneCurve> {
        let mut unscripted = None;
        for entry in text.split(';') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            match entry.split_once('=') {
                None => {
                    if unscripted.is_none() {
                        unscripted = ToneCurve::parse(entry);
                    }
                }
                Some((key, curve)) => {
                    let key = key.trim();
                    if key.len() != 4 || !key.bytes().all(|b| b.is_ascii_alphabetic()) {
                        continue;
                    }
                    if script.is_some_and(|s| s.trim().eq_ignore_ascii_case(key))
                        && let Some(tone) = ToneCurve::parse(curve)
                    {
                        return Some(tone);
                    }
                }
            }
        }
        unscripted
    }

    /// The shortest text [`ToneCurve::parse`] reads back to the same
    /// curve: a name where the curve is `smooth`, a contrast step or a
    /// sharp one, else its 16 numbers.
    pub fn to_text(&self, out: &mut String) {
        use core::fmt::Write as _;
        if *self == ToneCurve::SMOOTH {
            out.push_str("smooth");
        } else if let Some(step) =
            (ToneCurve::LIGHTEST..=ToneCurve::DARKEST).find(|s| ToneCurve::contrast(*s) == *self)
        {
            let _ = write!(out, "contrast:{step}");
        } else if let Some(threshold) = (1..=255u8).find(|t| ToneCurve::SMOOTH.sharp(*t) == *self) {
            let _ = write!(out, "sharp:{threshold}");
        } else {
            for (i, alpha) in self.alphas().iter().enumerate() {
                let _ = write!(out, "{}{alpha}", if i == 0 { "" } else { " " });
            }
        }
    }
}

#[cfg(test)]
#[path = "curve_text_tests.rs"]
mod tests;
