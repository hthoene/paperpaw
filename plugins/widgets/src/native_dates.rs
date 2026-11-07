//! A month laid out in weeks ([`MonthGrid`]); the calendar's arithmetic
//! is the SDK's (`paperpaw_sdk::time`), named here for the time widgets.

pub(crate) use paperpaw_sdk::time::{
    civil_from_days, day_of_year, days_from_civil, days_in_month, days_in_year, iso_week, weekday,
};

/// A month laid out in weeks: on which day of the first week it starts
/// (0: the week's first day), how many days it has, and over how many
/// weeks it reaches; weeks start on `week_start` (0: Sunday, 1: Monday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MonthGrid {
    pub year: i32,
    pub month: u8,
    pub offset: u8,
    pub days: u8,
    pub weeks: u8,
    pub week_start: u8,
}

impl MonthGrid {
    pub(crate) fn new(year: i32, month: u8, week_start: u8) -> Self {
        let offset = (weekday(year, month, 1) + 7 - week_start % 7) % 7;
        let days = days_in_month(year, month);
        Self {
            year,
            month,
            offset,
            days,
            weeks: (offset + days).div_ceil(7),
            week_start: week_start % 7,
        }
    }

    /// The month after it.
    pub(crate) fn next(&self) -> Self {
        let (year, month) = if self.month == 12 {
            (self.year + 1, 1)
        } else {
            (self.year, self.month + 1)
        };
        Self::new(year, month, self.week_start)
    }

    /// The day in week `week`, column `column` (0 to 6), as days from the
    /// month's first (negative before it, `days` and more after it).
    pub(crate) fn day_at(&self, week: u8, column: u8) -> i32 {
        i32::from(week) * 7 + i32::from(column) - i32::from(self.offset)
    }

    /// The week (row) day `day` (1 to `days`) is in.
    pub(crate) fn week_of(&self, day: u8) -> u8 {
        (self.offset + day.max(1) - 1) / 7
    }

    /// The weekday of column `column` (0: Sunday).
    pub(crate) fn weekday_of(&self, column: u8) -> u8 {
        (self.week_start + column) % 7
    }

    /// The ISO number of week `week` (the week of its Thursday).
    pub(crate) fn week_number(&self, week: u8) -> u8 {
        let thursday = (4 + 7 - self.week_start) % 7;
        let first = days_from_civil(self.year, self.month, 1);
        let (y, m, d) = civil_from_days(first + i64::from(self.day_at(week, thursday)));
        iso_week(y, m, d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_laid_out_in_weeks() {
        // October 2026 starts on a Thursday.
        let october = MonthGrid::new(2026, 10, 1);
        assert_eq!((october.offset, october.days, october.weeks), (3, 31, 5));
        let october = MonthGrid::new(2026, 10, 0);
        assert_eq!((october.offset, october.weeks), (4, 5));
        assert_eq!(october.day_at(0, 4), 0);
        // The 14th is in the third row, the 1st in the first.
        assert_eq!(october.week_of(14), 2);
        assert_eq!(october.week_of(1), 0);
        assert_eq!(october.week_of(31), 4);
        assert_eq!(october.weekday_of(0), 0);
        // February 2026 from Monday: four full weeks, nothing else.
        let february = MonthGrid::new(2026, 2, 1);
        assert_eq!((february.offset, february.days, february.weeks), (6, 28, 5));
        let february = MonthGrid::new(2027, 2, 1);
        assert_eq!((february.offset, february.weeks), (0, 4));
        // A leap February, from Sunday.
        let february = MonthGrid::new(2028, 2, 0);
        assert_eq!((february.offset, february.days, february.weeks), (2, 29, 5));
        // Six weeks: August 2026 from Monday starts on a Saturday.
        let august = MonthGrid::new(2026, 8, 1);
        assert_eq!((august.offset, august.weeks), (5, 6));
        // The year goes on in December.
        let next = MonthGrid::new(2026, 12, 1).next();
        assert_eq!((next.year, next.month), (2027, 1));
        // Week numbers of October 2026's rows: 40 to 44.
        let october = MonthGrid::new(2026, 10, 1);
        let weeks: [u8; 5] = core::array::from_fn(|w| october.week_number(w as u8));
        assert_eq!(weeks, [40, 41, 42, 43, 44]);
        // From Sunday, January 2027's first row is 2026's week 53.
        let january = MonthGrid::new(2027, 1, 0);
        assert_eq!(january.week_number(0), 53);
    }
}
