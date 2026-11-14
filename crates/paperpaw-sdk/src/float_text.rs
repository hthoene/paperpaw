//! Floats as text without `core`'s parser and formatter of floats: those
//! are some 36 KB of the firmware (#258), for a handful of numbers.
//!
//! What is written is what `f32`'s `Display` writes, digit for digit (a
//! page index's key is made of such texts and stays what it was); what
//! is read is as near as the float can say.

use core::cmp::Ordering;
use core::fmt;

/// Powers of ten an `f64` holds exactly.
const TENS: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// `value` times ten to the `scale`, in steps of powers an `f64` holds
/// exactly: its 53 bits keep the steps' rounding far below an `f32`'s
/// last place.
fn scaled(mut value: f64, scale: i32) -> f64 {
    // Beyond what an f32 holds either way: no endless steps.
    let mut scale = scale.clamp(-400, 400);
    while scale != 0 {
        let step = scale.unsigned_abs().min(22) as usize;
        let ten = TENS.get(step).copied().unwrap_or(1.0);
        if scale > 0 {
            value *= ten;
            scale -= step as i32;
        } else {
            value /= ten;
            scale += step as i32;
        }
    }
    value
}

/// `[+-]digits[.digits][e[+-]digits]`, `inf` or `NaN`, as [`write_f32`]
/// and `f32`'s `Display` write them: the float nearest to the number, as
/// `core` reads it, for a text of up to 19 digits (every one written here
/// has at most nine); of a longer one the first 19 are read.
pub(crate) fn parse_f32(text: &str) -> Option<f32> {
    let (negative, rest) = match text.as_bytes().first()? {
        b'-' => (true, text.get(1..)?),
        b'+' => (false, text.get(1..)?),
        _ => (false, text),
    };
    let sign = if negative { -1.0 } else { 1.0 };
    // In capitals or small letters, as `core` reads them.
    if rest.eq_ignore_ascii_case("inf") || rest.eq_ignore_ascii_case("infinity") {
        return Some(sign * f32::INFINITY);
    }
    if rest.eq_ignore_ascii_case("nan") {
        return Some(f32::NAN);
    }
    let (number, exponent) = match rest.split_once(['e', 'E']) {
        // Clamped so that counting digits cannot overflow it.
        Some((number, exponent)) => (number, exponent.parse::<i32>().ok()?.clamp(-10_000, 10_000)),
        None => (rest, 0),
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    // The first 19 digits, as a whole number, and where the point is.
    let mut mantissa: u64 = 0;
    let mut digits = 0;
    let mut scale = exponent;
    for (i, b) in whole.bytes().chain(fraction.bytes()).enumerate() {
        if !b.is_ascii_digit() {
            return None;
        }
        let in_fraction = i >= whole.len();
        if digits < 19 {
            if mantissa > 0 || b != b'0' {
                digits += 1;
            }
            mantissa = mantissa * 10 + u64::from(b - b'0');
            scale -= i32::from(in_fraction);
        } else if !in_fraction {
            scale += 1;
        }
    }
    let near = scaled(mantissa as f64, scale) as f32;
    Some(sign * settled(mantissa, scale, near))
}

/// The float `mantissa` times ten to the `scale` reads as. `near` is what
/// the `f64`'s steps gave: that float, or, for a number a hair from half
/// way between two floats, the one beside it; which of them is told in
/// whole numbers ([`reads_back`]).
fn settled(mantissa: u64, scale: i32, near: f32) -> f32 {
    // Nothing, or beyond every float by far: the steps are right (and the
    // whole numbers would not hold it).
    if mantissa == 0 || near.is_nan() || !(-70..=40).contains(&scale) {
        return near;
    }
    let fits = |float: f32| float > 0.0 && float.is_finite() && reads_back(mantissa, scale, float);
    let (up, down) = (near.next_up(), near.next_down());
    if fits(near) {
        near
    } else if fits(up) {
        up
    } else if fits(down) {
        down
    } else if near == f32::MAX {
        // Past half way above the largest float.
        f32::INFINITY
    } else if near.to_bits() == 1 {
        // Not half way up to the smallest one.
        0.0
    } else {
        near
    }
}

/// A whole number of 256 bits, the lowest word first: room for a float's
/// bounds times any power of ten its digits have.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Wide([u32; 8]);

impl Wide {
    fn of(number: u64) -> Self {
        Self([number as u32, (number >> 32) as u32, 0, 0, 0, 0, 0, 0])
    }

    /// Times ten to the `power`.
    fn times_ten_to(mut self, power: u32) -> Self {
        for _ in 0..power {
            let mut carry = 0;
            for word in &mut self.0 {
                let wide = u64::from(*word) * 10 + carry;
                *word = wide as u32;
                carry = wide >> 32;
            }
        }
        self
    }

    /// Times two to the `by`.
    fn shifted(mut self, by: u32) -> Self {
        let (words, bits) = ((by / 32) as usize, by % 32);
        let len = self.0.len();
        if words >= len {
            return Self::of(0);
        }
        self.0.copy_within(0..len - words, words);
        self.0.iter_mut().take(words).for_each(|word| *word = 0);
        if bits > 0 {
            let mut carry = 0;
            for word in &mut self.0 {
                let next = *word >> (32 - bits);
                *word = (*word << bits) | carry;
                carry = next;
            }
        }
        self
    }

    fn compare(&self, other: &Self) -> Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }
}

/// Whether `digits` times ten to the `last` reads as `value` (a positive
/// number) and no other float: in whole numbers, so that one half way to
/// the next float is told from one just short of it.
fn reads_back(digits: u64, last: i32, value: f32) -> bool {
    let bits = value.to_bits();
    let (field, fraction) = ((bits >> 23) & 0xff, bits & 0x7f_ffff);
    // `value` is `whole` times two to the `power`.
    let (whole, power) = if field == 0 {
        (fraction, -149)
    } else {
        (fraction | 0x80_0000, field as i32 - 150)
    };
    // Half way to its neighbours, in quarters of its last place: the one
    // below a power of two is half as far.
    let above = 4 * whole + 2;
    let below = if fraction == 0 && field > 1 {
        4 * whole - 1
    } else {
        4 * whole - 2
    };
    let quarters = power - 2;
    let scale = |number: u64, ten: bool, two: bool| {
        let number = Wide::of(number);
        let number = if ten {
            number.times_ten_to(last.unsigned_abs())
        } else {
            number
        };
        if two {
            number.shifted(quarters.unsigned_abs())
        } else {
            number
        }
    };
    let text = scale(digits, last >= 0, quarters < 0);
    let above = scale(u64::from(above), last < 0, quarters >= 0);
    let below = scale(u64::from(below), last < 0, quarters >= 0);
    // Half way reads as the even float of the two.
    let even = whole % 2 == 0;
    let not_past = |bound: &Wide, beyond: Ordering| match text.compare(bound) {
        Ordering::Equal => even,
        other => other != beyond,
    };
    not_past(&below, Ordering::Less) && not_past(&above, Ordering::Greater)
}

/// The fewest digits (at most nine, which tell any `f32` apart) that read
/// back as `value`, a positive number, and the power of ten of the last
/// of them.
fn shortest(value: f32) -> (u32, i32) {
    let exact = f64::from(value);
    // The power of ten of its first digit, from its power of two
    // (log10(2) is 1233/4096), at most one too low.
    let two = ((value.to_bits() >> 23) & 0xff) as i32 - 127;
    let mut first = (two * 1233) >> 12;
    if scaled(exact, -first) >= 10.0 {
        first += 1;
    }
    let mut found = (0, 0);
    for count in 1..=9 {
        let last = first - (count - 1);
        let at = scaled(exact, -last);
        let below = at as u32;
        // The nearer of the two beside it first; half way, the upper.
        let order = if at - f64::from(below) >= 0.5 {
            [below + 1, below]
        } else {
            [below, below + 1]
        };
        for digits in order {
            found = (digits, last);
            if reads_back(u64::from(digits), last, value) {
                // Rounded up to a one and noughts: the one alone.
                while found.0 % 10 == 0 && found.0 > 0 {
                    found = (found.0 / 10, found.1 + 1);
                }
                return found;
            }
        }
    }
    found
}

/// Writes the sign of `value` and says what else there is to write: its
/// size, or nothing where it is no number (`NaN`, `inf`: written here).
fn signed(out: &mut impl fmt::Write, value: f32) -> Result<Option<f32>, fmt::Error> {
    if value.is_nan() {
        out.write_str("NaN")?;
        return Ok(None);
    }
    if value.is_sign_negative() {
        out.write_char('-')?;
    }
    if value.is_infinite() {
        out.write_str("inf")?;
        return Ok(None);
    }
    Ok(Some(value.abs()))
}

/// `value` as `f32`'s `Display` writes it: the fewest digits that read
/// back as it, no exponent, `inf` and `NaN`.
pub(crate) fn write_f32(out: &mut impl fmt::Write, value: f32) -> fmt::Result {
    let Some(value) = signed(out, value)? else {
        return Ok(());
    };
    if value == 0.0 {
        return out.write_char('0');
    }
    let (mut rest, last) = shortest(value);
    let mut buffer = [b'0'; 10];
    let mut at = buffer.len();
    while rest > 0 && at > 0 {
        at -= 1;
        if let Some(digit) = buffer.get_mut(at) {
            *digit = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
    }
    let digits = buffer.get(at..).unwrap_or_default();
    // Where the point is, counted from the first digit.
    let point = digits.len() as i32 + last;
    if point <= 0 {
        out.write_str("0.")?;
        for _ in point..0 {
            out.write_char('0')?;
        }
    }
    for (i, digit) in digits.iter().enumerate() {
        if point > 0 && i as i32 == point {
            out.write_char('.')?;
        }
        out.write_char(char::from(*digit))?;
    }
    for _ in digits.len() as i32..point {
        out.write_char('0')?;
    }
    Ok(())
}

/// A float to write as [`write_f32`] writes it, for `format!`.
#[cfg(any(feature = "unstable", test))]
pub(crate) struct Shown(pub f32);

#[cfg(any(feature = "unstable", test))]
impl fmt::Display for Shown {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_f32(out, self.0)
    }
}

/// `value` with one digit after the point, as `{:.1}` writes an `f32`:
/// rounded to the nearer tenth, half way to the even one. From 10^15 on
/// (whole numbers all) the digits are those of [`write_f32`], which read
/// back as the same float but are not all of its digits.
pub(crate) fn write_tenths(out: &mut impl fmt::Write, value: f32) -> fmt::Result {
    let Some(value) = signed(out, value)? else {
        return Ok(());
    };
    if value >= 1e15 {
        write_f32(out, value)?;
        return out.write_str(".0");
    }
    // Ten times a float of 24 bits: exact in an f64, and so is the rest.
    let exact = f64::from(value) * 10.0;
    let mut tenths = exact as u64;
    let rest = exact - tenths as f64;
    if rest > 0.5 || (rest == 0.5 && tenths % 2 == 1) {
        tenths += 1;
    }
    write!(out, "{}.{}", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::string::{String, ToString};

    use super::*;

    fn tenths(value: f32) -> String {
        let mut text = String::new();
        write_tenths(&mut text, value).expect("a string takes it");
        text
    }

    /// Floats of every size and sign: one in 4,099 of all there are, and
    /// the ones texts are made of here (sizes, spacings, percents).
    fn samples() -> impl Iterator<Item = f32> {
        let spread = (0..=u32::MAX).step_by(4099).map(f32::from_bits);
        let usual = (0..200_000u32).flat_map(|n| {
            let n = n as f32;
            [n / 4.0, n / 10.0, n / 100.0, -n / 8.0, n * 0.05, n * 1.1]
        });
        let edges = [0.0, -0.0, f32::MIN_POSITIVE, f32::MAX, f32::MIN, 1e-45];
        spread.chain(usual).chain(edges)
    }

    /// What the page indexes' keys rest on (a key of other digits would
    /// lay every book out anew): each float as `core` writes it.
    #[test]
    fn floats_are_written_as_core_writes_them() {
        for value in samples() {
            assert_eq!(Shown(value).to_string(), value.to_string(), "{value:e}");
        }
        assert_eq!(Shown(f32::NAN).to_string(), "NaN");
        assert_eq!(Shown(f32::NEG_INFINITY).to_string(), "-inf");
        // Seven digits a hair short of half way to the next float still
        // read as this one: no eighth is written.
        let hard = f32::from_bits(0x15ae_43fd);
        assert_eq!(
            Shown(hard).to_string(),
            "0.00000000000000000000000007038531"
        );
        assert_eq!(Shown(hard).to_string(), hard.to_string());
    }

    #[test]
    fn what_is_written_reads_back_as_the_same_float() {
        for value in samples().filter(|value| !value.is_nan()) {
            let read = parse_f32(&Shown(value).to_string()).expect("a number");
            assert_eq!(read.to_bits(), value.to_bits(), "{value:e}");
        }
        assert!(parse_f32("NaN").is_some_and(f32::is_nan));
        // A hair short of half way to the next float: the f64's steps alone
        // read the float beside it (the one float in all that they did).
        for bits in [0x15ae_43fd_u32, 0x95ae_43fd] {
            let hard = f32::from_bits(bits);
            let read = parse_f32(&Shown(hard).to_string()).expect("a number");
            assert_eq!(read.to_bits(), bits);
        }
    }

    /// Numbers at the floats' ends and half way between two floats, in up
    /// to 19 digits: the float `core` reads.
    #[test]
    fn hard_numbers_are_read_as_core_reads_them() {
        let mut texts: alloc::vec::Vec<String> = [
            "3.4028235e38",
            "3.4028236e38",
            "3.40282357e38",
            "3.4028235677973366e38",
            "3.4028235677973367e38",
            "1e39",
            "1e-46",
            "7e-46",
            "7.006492321624085e-46",
            "7.006492321624086e-46",
            "1.4e-45",
            "2.1e-45",
            "2.1019476964872256e-45",
            "1.1754942e-38",
            "1.17549435e-38",
            "1.1754943508222875e-38",
            "16777217",
            "16777217.0000000001",
            "16777218.99999999",
            "16777219",
            "9007199791611905",
            "0.00000000000000000000000007038531",
            "7.0385315e-26",
            "70385314e-33",
            "1e-70",
            "1e40",
            "9999999999999999999e-64",
            "9999999999999999999e20",
            "0.000",
            "0e99",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        // Half way between two floats, a hair to either side of it, at
        // every power of two an f32 has: the 17 digits an f64 writes for
        // it, the next number of as many digits, and both with a nineteenth.
        for field in 0..=254_u32 {
            for fraction in [0, 1, 0x40_0000, 0x7f_ffff] {
                let float = f32::from_bits((field << 23) | fraction);
                let half = (f64::from(float) + f64::from(float.next_up())) / 2.0;
                if !half.is_finite() {
                    continue;
                }
                let text = format!("{half:e}");
                let (digits, exponent) = text.split_once('e').expect("an exponent");
                let digits: String = digits.chars().filter(char::is_ascii_digit).collect();
                let exponent: i32 = exponent.parse().expect("a number");
                let whole: u64 = digits.parse().expect("digits");
                let scale = exponent - (digits.len() as i32 - 1);
                for bump in [0, 1] {
                    texts.push(format!("{}e{scale}", whole + bump));
                    texts.push(format!("{}e{}", (whole + bump) * 100 - 1, scale - 2));
                }
            }
        }
        for text in texts {
            let core: f32 = text.parse().expect("a number");
            let ours = parse_f32(&text).expect("a number");
            assert_eq!(ours.to_bits(), core.to_bits(), "{text}");
        }
    }

    /// A position file's percent, as `{:.1}` wrote it.
    #[test]
    fn tenths_are_written_as_core_writes_them() {
        for value in samples().filter(|value| value.abs() < 1e15) {
            assert_eq!(tenths(value), format!("{value:.1}"), "{value:e}");
        }
        // Half way goes to the even tenth, a little above it up.
        assert_eq!(tenths(0.25), "0.2");
        assert_eq!(tenths(0.75), "0.8");
        assert_eq!(tenths(99.95), "99.9");
        assert_eq!(tenths(-0.04), "-0.0");
        assert_eq!(tenths(f32::NAN), "NaN");
        assert_eq!(tenths(f32::INFINITY), "inf");
        // Whole numbers too large to matter: digits that read back the same.
        assert_eq!(tenths(1e15), "1000000000000000.0");
    }

    /// Texts are read as `core` reads them: the same float for a number
    /// of up to eight digits after its point (styles, settings, files),
    /// the same texts refused.
    #[test]
    fn numbers_are_read_as_core_reads_them() {
        for n in (0..3_000_000u32).step_by(7) {
            for places in [0, 1, 2, 3, 5, 8] {
                let text = format!("{}.{:0places$}", n / 1000, n % 1000);
                let text = if places == 0 { n.to_string() } else { text };
                let core: f32 = text.parse().expect("a number");
                assert_eq!(
                    parse_f32(&text).map(f32::to_bits),
                    Some(core.to_bits()),
                    "{text}"
                );
            }
        }
        for text in [
            "1.5", "-1.5", "+1.5", ".5", "5.", "1e3", "1E-3", "1.5e+2", "-0", "inf", "-inf",
            "Infinity", "INF", "nan", "NaN", "", "-", "+", ".", "e5", "1e", "1e+", "1.5px", " 1",
            "1 ", "1_0", "0x10", "1..2", "1.2.3", "--1", "infinit", "na",
        ] {
            let core = text.parse::<f32>().ok();
            let ours = parse_f32(text);
            assert_eq!(ours.is_some(), core.is_some(), "{text:?}");
            if let (Some(ours), Some(core)) = (ours, core) {
                assert!(ours == core || (ours.is_nan() && core.is_nan()), "{text:?}");
            }
        }
    }
}
