//! Numbers as people read them in the language shown: its decimal mark
//! (`2.3` or `2,3`), for every plugin alike.

use alloc::format;
use alloc::string::String;

/// The translation that holds the language's decimal mark.
pub const DECIMAL_MARK: &str = "core.decimal";

/// `units` hundredths, tenths or other parts (`decimals` digits after the
/// mark) written with `mark`: `fixed(42, 1, ",")` is `4,2`,
/// `fixed(-5, 2, ".")` is `-0.05`.
pub fn fixed(units: i64, decimals: u32, mark: &str) -> String {
    if decimals == 0 {
        return format!("{units}");
    }
    let scale = 10u64.saturating_pow(decimals);
    let sign = if units < 0 { "-" } else { "" };
    let magnitude = units.unsigned_abs();
    let width = decimals as usize;
    format!(
        "{sign}{}{mark}{:0width$}",
        magnitude / scale,
        magnitude % scale
    )
}

/// A number as written with a point (`1.5`, `-.25`, `1e2`) and nothing
/// around it; `None` for anything else. Not `core`'s parser of floats,
/// which is 16 KB of the firmware: the same float for up to eight digits
/// after the point, within a unit of its last place for longer ones.
#[must_use]
pub fn parse_f32(text: &str) -> Option<f32> {
    crate::float_text::parse_f32(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_numbers() {
        assert_eq!(fixed(42, 1, ","), "4,2");
        assert_eq!(fixed(401, 2, "."), "4.01");
        assert_eq!(fixed(-5, 2, "."), "-0.05");
        assert_eq!(fixed(7, 0, "."), "7");
        assert_eq!(fixed(i64::MIN, 3, "."), "-9223372036854775.808");
    }
}
