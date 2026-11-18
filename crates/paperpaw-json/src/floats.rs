//! A JSON number read without `core`'s parser of floats, which is 16 KB
//! of the firmware (#258) for the few numbers web services answer with.
//!
//! The value is the one `core` reads, bit for bit: a short number in one
//! exact step, any other by whole-number arithmetic as long as it takes
//! (a few words on the heap, 1 KB at the very most, let go at once).

use alloc::vec::Vec;
use core::cmp::Ordering;

/// Powers of ten an `f64` holds exactly.
const TENS: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// The digits that count: no point half way between two floats has more
/// (767), so what follows them only says "and a little more".
const DIGITS: usize = 768;

/// A sign, if there is one, and what follows it.
fn signed(text: &str) -> Option<(bool, &str)> {
    match text.as_bytes().first()? {
        b'-' => Some((true, text.get(1..)?)),
        b'+' => Some((false, text.get(1..)?)),
        _ => Some((false, text)),
    }
}

/// An exponent's whole number; one beyond any float's as a million.
fn exponent(text: &str) -> Option<i64> {
    let (negative, digits) = signed(text)?;
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        value = (value * 10 + i64::from(b - b'0')).min(1_000_000);
    }
    Some(if negative { -value } else { value })
}

/// A whole number of as many words as it was made with, the lowest first.
struct Big(Vec<u32>);

impl Big {
    /// Nought, in `words` words; `None` where there is no room for them.
    fn zero(words: usize) -> Option<Self> {
        let mut made = Vec::new();
        made.try_reserve_exact(words).ok()?;
        made.resize(words, 0);
        Some(Self(made))
    }

    /// Times `by`, plus `add`.
    fn times_plus(&mut self, by: u32, add: u32) {
        let mut carry = u64::from(add);
        for word in &mut self.0 {
            let wide = u64::from(*word) * u64::from(by) + carry;
            *word = wide as u32;
            carry = wide >> 32;
        }
    }

    /// Times ten to the `power`.
    fn times_ten_to(&mut self, mut power: u32) {
        while power >= 9 {
            self.times_plus(1_000_000_000, 0);
            power -= 9;
        }
        for _ in 0..power {
            self.times_plus(10, 0);
        }
    }

    /// How many bits it takes to write.
    fn bits(&self) -> u32 {
        let words = self.0.iter().rposition(|word| *word != 0);
        words.map_or(0, |top| {
            let word = self.0.get(top).copied().unwrap_or(0);
            top as u32 * 32 + (32 - word.leading_zeros())
        })
    }

    /// Times two to the `by`.
    fn shift_up(&mut self, by: u32) {
        let (words, bits) = ((by / 32) as usize, by % 32);
        let len = self.0.len();
        if words >= len {
            self.0.fill(0);
            return;
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
    }

    /// Halved, the odd bit dropped.
    fn halve(&mut self) {
        let mut carry = 0;
        for word in self.0.iter_mut().rev() {
            let next = *word << 31;
            *word = (*word >> 1) | carry;
            carry = next;
        }
    }

    /// Against one of as many words.
    fn compare(&self, other: &Self) -> Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }

    /// Less `other`, which is no more than it.
    fn less(&mut self, other: &Self) {
        let mut borrow = false;
        for (word, take) in self.0.iter_mut().zip(&other.0) {
            let (first, under) = word.overflowing_sub(*take);
            let (second, under_too) = first.overflowing_sub(u32::from(borrow));
            *word = second;
            borrow = under || under_too;
        }
    }

    fn is_zero(&self) -> bool {
        self.0.iter().all(|word| *word == 0)
    }
}

/// The digits of a number that count and the power of ten of the last.
struct Digits<'a> {
    whole: &'a str,
    fraction: &'a str,
    /// How many count: those from the first that is not nought, at most
    /// [`DIGITS`].
    count: usize,
    /// The power of ten of the last that counts.
    power: i64,
    /// Whether one after them is not nought.
    more: bool,
}

impl<'a> Digits<'a> {
    /// `None` where a sign is not a digit.
    fn of(whole: &'a str, fraction: &'a str, exponent: i64) -> Option<Self> {
        let (mut count, mut power, mut more) = (0, exponent, false);
        for (i, b) in whole.bytes().chain(fraction.bytes()).enumerate() {
            if !b.is_ascii_digit() {
                return None;
            }
            let in_fraction = i >= whole.len();
            if count < DIGITS {
                if count > 0 || b != b'0' {
                    count += 1;
                }
                power -= i64::from(in_fraction);
            } else {
                more |= b != b'0';
                power += i64::from(!in_fraction);
            }
        }
        Some(Self {
            whole,
            fraction,
            count,
            power,
            more,
        })
    }

    /// Those that count, the first first.
    fn counted(&self) -> impl Iterator<Item = u8> + '_ {
        let all = self.whole.bytes().chain(self.fraction.bytes());
        all.skip_while(|b| *b == b'0')
            .take(self.count)
            .map(|b| b - b'0')
    }

    /// The first 19 as a whole number, and the power of ten of the last
    /// of those.
    fn start(&self) -> (u64, i64) {
        let taken = self.count.min(19);
        let number = self
            .counted()
            .take(taken)
            .fold(0_u64, |number, digit| number * 10 + u64::from(digit));
        (number, self.power + (self.count - taken) as i64)
    }
}

/// `number` times two to the `power`, and a little more where `more`, as
/// the nearest float (half way: the even one). `number` is 2^62 or more.
fn nearest(number: u64, power: i64, more: bool) -> f64 {
    let lead = number.leading_zeros();
    let number = number << lead;
    // The power of two of its first bit.
    let first = power - i64::from(lead) + 63;
    if first > 1023 {
        return f64::INFINITY;
    }
    // The bits past the float's 53, or past those a tiny one has left.
    let dropped = if first >= -1022 {
        11
    } else {
        11 + (-1022 - first)
    };
    let (kept, half, below) = match dropped {
        65.. => (0, false, true),
        64 => (0, number >> 63 == 1, number << 1 != 0),
        _ => (
            number >> dropped,
            (number >> (dropped - 1)) & 1 == 1,
            number & ((1 << (dropped - 1)) - 1) != 0,
        ),
    };
    let up = half && (more || below || kept & 1 == 1);
    // `kept` holds the leading bit, which counts the exponent up by one,
    // as does rounding up to the next power of two.
    let exponent = if first >= -1022 {
        (first + 1022) as u64
    } else {
        0
    };
    let bits = (exponent << 52) + kept + u64::from(up);
    if bits >= 0x7ff << 52 {
        return f64::INFINITY;
    }
    f64::from_bits(bits)
}

/// The float nearest the digits, by whole numbers: the digits (times
/// their power of ten) over the power of ten below them, 64 bits of it
/// and whether anything is left. `None` where there is no room.
fn exact(digits: &Digits<'_>) -> Option<f64> {
    let count = digits.count as u64;
    let power = digits.power.unsigned_abs();
    // Bits a number of so many digits has at most (a digit is 3.33).
    let most = |digits: u64| digits * 10 / 3 + 2;
    let bits = if digits.power >= 0 {
        most(count + power).max(64)
    } else {
        most(count).max(most(power) + 63)
    };
    let words = usize::try_from(bits / 32 + 2).ok()?;
    let mut over = Big::zero(words)?;
    let mut under = Big::zero(words)?;
    for digit in digits.counted() {
        over.times_plus(10, u32::from(digit));
    }
    under.times_plus(0, 1);
    let ten = u32::try_from(power).ok()?;
    if digits.power >= 0 {
        over.times_ten_to(ten);
    } else {
        under.times_ten_to(ten);
    }
    // 63 bits more above than below: the quotient is of 63 or 64 bits.
    let shift = i64::from(under.bits()) + 63 - i64::from(over.bits());
    let by = u32::try_from(shift.unsigned_abs()).ok()?;
    if shift >= 0 {
        over.shift_up(by);
        under.shift_up(63);
    } else {
        under.shift_up(by + 63);
    }
    let mut quotient = 0_u64;
    for bit in (0..64).rev() {
        if over.compare(&under) != Ordering::Less {
            over.less(&under);
            quotient |= 1 << bit;
        }
        under.halve();
    }
    Some(nearest(quotient, -shift, digits.more || !over.is_zero()))
}

/// The float near the digits where [`exact`] has no room: their first 19
/// in steps of powers of ten, some places of the 16th digit off at most.
fn nearly(digits: &Digits<'_>) -> f64 {
    let (start, power) = digits.start();
    let mut value = start as f64;
    let mut scale = power;
    while scale != 0 {
        let step = scale.unsigned_abs().min(22) as usize;
        let ten = TENS.get(step).copied().unwrap_or(1.0);
        if scale > 0 {
            value *= ten;
            scale -= step as i64;
        } else {
            value /= ten;
            scale += step as i64;
        }
    }
    value
}

/// `[+-]digits[.digits][e[+-]digits]` as `core` reads it, and the very
/// float `core` makes of it.
pub(crate) fn number(text: &str) -> Option<f64> {
    let (negative, rest) = signed(text)?;
    let (number, exponent) = match rest.split_once(['e', 'E']) {
        Some((number, power)) => (number, exponent(power)?),
        None => (rest, 0),
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let digits = Digits::of(whole, fraction, exponent)?;
    // As a fraction of one: the power of ten above its first digit.
    let above = digits.power + digits.count as i64;
    let value = if digits.count == 0 || above < -323 {
        // Nought, or below half the least float.
        0.0
    } else if above > 309 {
        f64::INFINITY
    } else if digits.count <= 15 && digits.power.unsigned_abs() <= 22 {
        // A whole number below 2^53 and a power of ten, both exact: one
        // step, rounded once.
        let (start, _) = digits.start();
        let ten = TENS
            .get(digits.power.unsigned_abs() as usize)
            .copied()
            .unwrap_or(1.0);
        if digits.power < 0 {
            start as f64 / ten
        } else {
            start as f64 * ten
        }
    } else {
        exact(&digits).unwrap_or_else(|| nearly(&digits))
    };
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::string::{String, ToString};

    use super::*;

    fn same(text: &str) {
        let core: f64 = text.parse().expect("a number");
        assert_eq!(
            number(text).map(f64::to_bits),
            Some(core.to_bits()),
            "{text}"
        );
    }

    /// Numbers that are no better than chance, the same in every run.
    struct Chance(u64);

    impl Chance {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, limit: u64) -> u64 {
            self.next() % limit
        }
    }

    /// What plugins rest on (a time, a size, a page, a percentage): the
    /// value `core`'s parser read, bit for bit.
    #[test]
    fn usual_numbers_are_read_as_core_reads_them() {
        let mut n: u64 = 1;
        while n < 1 << 53 {
            for whole in [n - 1, n, n + 1, n * 3 / 2] {
                same(&whole.to_string());
                same(&format!("-{whole}"));
            }
            n *= 2;
        }
        for n in (0..2_000_000_000u64).step_by(7919) {
            for places in [1, 2, 3, 6, 9] {
                same(&format!("{}.{:0places$}", n / 1000, n % 1000));
                same(&format!("0.{n:015}"));
                same(&format!("{n}e-{places}"));
                same(&format!("{n}E+{}", places + 13));
            }
        }
        for text in [
            "0",
            "-0",
            "0.45",
            "1700000000",
            "1e22",
            "1e-22",
            "123456789012345e-22",
            "52.520006599999996",
            "0.10e-24",
        ] {
            same(text);
        }
    }

    /// Long ones, large ones and tiny ones are the same float too.
    #[test]
    fn other_numbers_are_read_as_core_reads_them() {
        for text in [
            "0.1234567890123456789",
            "12345678901234567890123",
            "1.7976931348623157e308",
            "1.7976931348623158e308",
            "1.7976931348623159e308",
            "179769313486231580793728971405303415079934132710037826936173778980444968292764750946649017977587207096330286416692887910946555547851940402630657488671505820681908902000708383676273854845817711531764475730270069855571366959622842914819860834936475292719074168444365510704342711559699508093042880177904174497791.999",
            "179769313486231580793728971405303415079934132710037826936173778980444968292764750946649017977587207096330286416692887910946555547851940402630657488671505820681908902000708383676273854845817711531764475730270069855571366959622842914819860834936475292719074168444365510704342711559699508093042880177904174497792",
            "2.2250738585072014e-308",
            "2.2250738585072011e-308",
            "4.9e-324",
            "5e-324",
            "2.4703282292062327e-324",
            "2.4703282292062328e-324",
            "2.5e-324",
            "7.4e-324",
            "7.5e-324",
            "1e300",
            "1e-300",
            "1e308",
            "1e309",
            "1e-323",
            "1e-324",
            "9007199254740993",
            "9007199254740992.999",
            "9007199254740993.000000000000000000000000000001",
            "123456789.123456789e40",
            "0.000000000000000000000000000000000000000000001e350",
            "1000000000000000000000000000000000000000000000e-350",
        ] {
            same(text);
        }
        assert_eq!(number("1e999"), Some(f64::INFINITY));
        assert_eq!(number("1e-999"), Some(0.0));
        assert_eq!(number("1e99999999999999999999"), Some(f64::INFINITY));
        assert_eq!(number("0e99999999999999999999"), Some(0.0));
        assert_eq!(number("-0").map(f64::to_bits), Some((-0.0f64).to_bits()));
        let mut chance = Chance(0x9e37_79b9_7f4a_7c15);
        for _ in 0..200_000 {
            let digits = 1 + chance.below(40) as usize;
            let mut text = String::new();
            for at in 0..digits {
                if at > 0 && chance.below(12) == 0 {
                    text.push('.');
                }
                text.push(char::from(b'0' + chance.below(10) as u8));
            }
            let text: String = match text.split_once('.') {
                // One point at most.
                Some((whole, rest)) => format!("{whole}.{}", rest.replace('.', "")),
                None => text,
            };
            let power = chance.below(700) as i64 - 350;
            same(&format!("{text}e{power}"));
            same(&text);
        }
    }

    /// `a` and `b`, written with as many digits after their points, added.
    fn sum(a: &str, b: &str) -> String {
        let width = a.len().max(b.len());
        let (a, b) = (format!("{a:0>width$}"), format!("{b:0>width$}"));
        let mut digits: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        let mut carry = 0;
        for (x, y) in a.bytes().rev().zip(b.bytes().rev()) {
            if x == b'.' {
                digits.push(b'.');
                continue;
            }
            let both = (x - b'0') + (y - b'0') + carry;
            digits.push(b'0' + both % 10);
            carry = both / 10;
        }
        digits.push(b'0' + carry);
        digits.reverse();
        String::from_utf8(digits).expect("digits")
    }

    /// Half of a number written with a point, a digit longer.
    fn half(text: &str) -> String {
        let mut out = String::new();
        let mut rest = 0;
        for b in text.bytes().chain(*b"0") {
            if b == b'.' {
                out.push('.');
                continue;
            }
            let digit = rest * 10 + (b - b'0');
            out.push(char::from(b'0' + digit / 2));
            rest = digit % 2;
        }
        out
    }

    /// Half way between two floats (the even one, as `core` has it), a
    /// little above and a little below, written out in full: the 768
    /// digits that count and whatever follows them.
    #[test]
    fn half_way_between_two_floats_is_read_as_core_reads_it() {
        let mut chance = Chance(0x2545_f491_4f6c_dd1d);
        let mut floats: alloc::vec::Vec<f64> = (0..300)
            .map(|_| f64::from_bits(chance.next() & (u64::MAX >> 1)))
            .filter(|float| float.is_finite())
            .collect();
        floats.extend([
            0.0,
            5e-324,
            1e-323,
            2.225_073_858_507_201e-308,
            f64::MIN_POSITIVE,
            1.0,
            9_007_199_254_740_992.0,
            1e22,
            1e23,
            f64::MAX / 2.0,
        ]);
        for float in floats {
            let next = f64::from_bits(float.to_bits() + 1);
            // Exact: a float has at most 1,074 digits after its point.
            let step = half(&format!("{:.1074}", next - float));
            let way = sum(&format!("{float:.1074}0"), &step);
            same(&way);
            same(&format!("{way}1"));
            same(&format!("{way}0000"));
            let last = way.rfind(|c: char| c.is_ascii_digit() && c != '0');
            let last = last.expect("not nought");
            let lower = way.as_bytes()[last] - 1;
            same(&format!(
                "{}{}{}",
                &way[..last],
                char::from(lower),
                &way[last + 1..]
            ));
            same(&format!("{way}e-3"));
        }
    }

    /// Without room for the whole numbers, the float is near enough.
    #[test]
    fn a_number_read_without_room_is_nearly_the_same() {
        for text in [
            "0.1234567890123456789",
            "12345678901234567890123",
            "1.7976931348623157e308",
            "2.2250738585072014e-308",
            "1e300",
            "123456789.123456789e40",
        ] {
            let core: f64 = text.parse().expect("a number");
            let (number, power) = text.split_once('e').unwrap_or((text, "0"));
            let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
            let digits =
                Digits::of(whole, fraction, power.parse().expect("a power")).expect("digits");
            let ours = nearly(&digits);
            assert!(
                (ours - core).abs() <= core.abs() * 1e-14,
                "{text}: {ours:e}"
            );
        }
    }

    /// The same texts are numbers, the same are none.
    #[test]
    fn the_same_texts_are_numbers_as_before() {
        for text in [
            "1", "-1", "+1", "1.", ".5", "-.5", "1.5e3", "1E3", "1e+3", "1e-3", "", "-", "+", ".",
            "-.", "e5", "1e", "1e+", "1e-", "1e1.5", "1.2.3", "1-2", "--1", "1+", "1e5e5", "+-1",
            "1 ", " 1", "1,5", "0x1",
        ] {
            assert_eq!(
                number(text).is_some(),
                text.parse::<f64>().is_ok(),
                "{text:?}"
            );
        }
    }
}
