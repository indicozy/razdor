//! Game time, as in the original: minutes since year 0, month 1, day 0, 00:00, with 30-day
//! months and 12-month years (`docs/reference/dtm-format.md`, *Game clock*). Time only
//! advances while the army travels or waits.
//!
//! The on-screen calendar of the original counts days from 0 (`5 месяц, 29 день` is followed
//! by `6 месяц, 0 день`) and months from 1, and shows whole hours only (video notes, §1).

pub const MINUTES_PER_HOUR: u64 = 60;
pub const MINUTES_PER_DAY: u64 = 24 * MINUTES_PER_HOUR;
pub const DAYS_PER_MONTH: u64 = 30;
pub const MONTHS_PER_YEAR: u64 = 12;
pub const DAYS_PER_YEAR: u64 = DAYS_PER_MONTH * MONTHS_PER_YEAR;

/// Hour of the daily report (income, wages, unpaid units): noon, as in the footage.
pub const REPORT_HOUR: u64 = 12;

/// A daily moment crossed while time passes. The value is the absolute day index
/// ([`Clock::day_index`]) of the day it happened on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// 00:00: villages refill their tribute.
    Midnight(u64),
    /// 12:00: income, wages and the daily report.
    Noon(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Clock {
    /// Minutes since year 0, month 1, day 0, 00:00.
    minutes: f64,
}

impl Clock {
    pub fn at_minutes(minutes: u64) -> Self {
        Clock { minutes: minutes as f64 }
    }

    /// A calendar moment: `month` 1..=12, `day` 0..=29.
    pub fn at(year: u64, month: u64, day: u64, hour: u64) -> Self {
        let days = year * DAYS_PER_YEAR + (month.clamp(1, 12) - 1) * DAYS_PER_MONTH + day.min(DAYS_PER_MONTH - 1);
        Clock::at_minutes(days * MINUTES_PER_DAY + hour * MINUTES_PER_HOUR)
    }

    /// Start of the built-in demo: year 1200, month 4, day 0, 08:00 (our choice).
    pub fn demo_start() -> Self {
        Clock::at(1200, 4, 0, 8)
    }

    pub fn total_minutes(&self) -> f64 {
        self.minutes
    }

    /// Sets the time without the day's moments (the AI's tick reads the moment of each of
    /// its arrivals, `rules::ai`).
    pub(crate) fn set_total_minutes(&mut self, minutes: f64) {
        self.minutes = minutes;
    }

    fn whole(&self) -> u64 {
        self.minutes as u64
    }

    /// Days since year 0.
    pub fn day_index(&self) -> u64 {
        self.whole() / MINUTES_PER_DAY
    }

    pub fn year(&self) -> u64 {
        self.day_index() / DAYS_PER_YEAR
    }

    /// 1..=12.
    pub fn month(&self) -> u64 {
        self.day_index() / DAYS_PER_MONTH % MONTHS_PER_YEAR + 1
    }

    /// Day of the month, 0..=29 as the original shows it.
    pub fn day(&self) -> u64 {
        self.day_index() % DAYS_PER_MONTH
    }

    pub fn hour(&self) -> u64 {
        self.whole() % MINUTES_PER_DAY / MINUTES_PER_HOUR
    }

    pub fn minute(&self) -> u64 {
        self.whole() % MINUTES_PER_HOUR
    }

    /// Advance and return the midnights and noons crossed, in order. A moment is crossed
    /// when the clock moves from before it to at or after it.
    pub fn advance(&mut self, minutes: f64) -> Vec<Tick> {
        let before = self.whole();
        self.minutes += minutes.max(0.0);
        let after = self.whole();
        let mut ticks = Vec::new();
        // Half-day steps: even ones are midnights, odd ones noons.
        let half = MINUTES_PER_DAY / 2;
        let first = before / half + 1;
        let last = after / half;
        for k in first..=last {
            let day = k / 2;
            ticks.push(if k % 2 == 0 { Tick::Midnight(day) } else { Tick::Noon(day) });
        }
        debug_assert_eq!(REPORT_HOUR * MINUTES_PER_HOUR, half);
        ticks
    }

    /// "1204, month 5, day 19, 11 h".
    pub fn label(&self) -> String {
        Self::label_of(self.year(), self.month(), self.day(), self.hour())
    }

    /// [`Clock::label`] for these parts (the bar sizes its date panel from the widest one).
    pub fn label_of(year: u64, month: u64, day: u64, hour: u64) -> String {
        crate::trf!("{year}, month {month}, day {day}, {hour} h", year = year, month = month, day = day, hour = hour)
    }

    /// The original's wording: "1204 год, 5 месяц, 19 день, 11 час".
    pub fn label_ru(&self) -> String {
        format!("{} год, {} месяц, {} день, {} час", self.year(), self.month(), self.day(), self.hour())
    }
}

/// A travel or wait duration in whole hours, as the original's "time left on the path":
/// "less than an hour", "1 h", "5 h", "2 d 3 h".
pub fn duration_label(minutes: f64) -> String {
    let hours = (minutes / MINUTES_PER_HOUR as f64).round() as u64;
    match hours {
        0 => crate::i18n::tr("less than an hour").to_string(),
        h if h < 24 => crate::trf!("{h} h", h),
        h if h % 24 == 0 => crate::trf!("{d} d", d = h / 24),
        h => crate::trf!("{d} d {h} h", d = h / 24, h = h % 24),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_fields_and_labels() {
        // The РК3 start: 624 354 300 minutes = year 1204, month 5, day 19 (0-based), 09:00.
        let c = Clock::at_minutes(624_354_300);
        assert_eq!((c.year(), c.month(), c.day(), c.hour(), c.minute()), (1204, 5, 19, 9, 0));
        assert_eq!(c.label(), "1204, month 5, day 19, 9 h");
        assert_eq!(c.label_ru(), "1204 год, 5 месяц, 19 день, 9 час");
        assert_eq!(Clock::at(1204, 5, 19, 9), c);
        assert_eq!(Clock::demo_start().label(), "1200, month 4, day 0, 8 h");
    }

    #[test]
    fn months_have_thirty_days_and_years_twelve_months() {
        let mut c = Clock::at(1204, 5, 29, 23);
        c.advance(60.0);
        assert_eq!((c.month(), c.day(), c.hour()), (6, 0, 0));
        let mut c = Clock::at(1204, 12, 29, 23);
        c.advance(60.0);
        assert_eq!((c.year(), c.month(), c.day()), (1205, 1, 0));
    }

    #[test]
    fn advance_reports_midnights_and_noons_in_order() {
        let mut c = Clock::at(1204, 5, 3, 8);
        let d = c.day_index();
        assert_eq!(c.advance(3.0 * 60.0), vec![], "08:00 -> 11:00");
        assert_eq!(c.advance(60.0), vec![Tick::Noon(d)], "exactly 12:00 counts");
        assert_eq!(c.advance(0.0), vec![]);
        assert_eq!(c.advance(12.0 * 60.0), vec![Tick::Midnight(d + 1)]);
        assert_eq!(
            c.advance(2.0 * 24.0 * 60.0),
            vec![Tick::Noon(d + 1), Tick::Midnight(d + 2), Tick::Noon(d + 2), Tick::Midnight(d + 3)]
        );
        assert_eq!(c.advance(-5.0), vec![], "time never runs backwards");
    }

    #[test]
    fn durations_in_hours() {
        assert_eq!(duration_label(20.0), "less than an hour");
        assert_eq!(duration_label(95.0), "2 h");
        assert_eq!(duration_label(48.0 * 60.0), "2 d");
        assert_eq!(duration_label(27.0 * 60.0), "1 d 3 h");
    }
}
