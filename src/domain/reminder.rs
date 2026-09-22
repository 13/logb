use chrono::{Datelike, Days, Months, NaiveDate, Weekday};

/// A reminder that watches a date or a counter target, and is closed by marking it done.
pub const KIND_SERVICE: &str = "service";
/// A reminder to record a counter reading at a regular interval. Nobody marks it done: logging
/// any entry with a counter value is what satisfies it (see `reading_status`).
pub const KIND_READING: &str = "reading";

/// The largest `every_n` accepted, in either unit. Five years of months is already far past
/// anything a "log the reading" habit means.
pub const MAX_EVERY: u32 = 60;

/// A recurrence pinned to the calendar instead of measured from completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarSchedule {
    Daily,
    Weekly(Weekday),
    Monthly(Option<u32>), // None means the last day of each month.
    Yearly(u32, u32),
}

impl CalendarSchedule {
    pub fn parse(raw: &str) -> Option<Self> {
        let parts: Vec<_> = raw.split(':').collect();
        match parts.as_slice() {
            ["daily"] => Some(Self::Daily),
            ["weekly", day] => Some(Self::Weekly(match day.parse::<u32>().ok()? {
                1 => Weekday::Mon, 2 => Weekday::Tue, 3 => Weekday::Wed, 4 => Weekday::Thu,
                5 => Weekday::Fri, 6 => Weekday::Sat, 7 => Weekday::Sun, _ => return None,
            })),
            ["monthly", "last"] => Some(Self::Monthly(None)),
            ["monthly", day] => Some(Self::Monthly(Some(day.parse().ok().filter(|d| (1..=31).contains(d))?))),
            ["yearly", month, day] => Some(Self::Yearly(
                month.parse().ok().filter(|m| (1..=12).contains(m))?,
                day.parse().ok().filter(|d| (1..=31).contains(d))?,
            )),
            _ => None,
        }
    }

    fn clamped(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let next = first.checked_add_months(Months::new(1))?;
        let last = next.pred_opt()?.day();
        NaiveDate::from_ymd_opt(year, month, day.min(last))
    }

    /// The first scheduled date strictly after `date`.
    pub fn next_after(self, date: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Daily => date.succ_opt(),
            Self::Weekly(day) => {
                let delta = (7 + i64::from(day.num_days_from_monday())
                    - i64::from(date.weekday().num_days_from_monday())) % 7;
                date.checked_add_days(Days::new(if delta == 0 { 7 } else { delta as u64 }))
            }
            Self::Monthly(day) => {
                let wanted = day.unwrap_or(31);
                let this = Self::clamped(date.year(), date.month(), wanted)?;
                if this > date { Some(this) } else {
                    let next = date.with_day(1)?.checked_add_months(Months::new(1))?;
                    Self::clamped(next.year(), next.month(), wanted)
                }
            }
            Self::Yearly(month, day) => {
                let this = Self::clamped(date.year(), month, day)?;
                if this > date { Some(this) } else { Self::clamped(date.year() + 1, month, day) }
            }
        }
    }

    /// The scheduled date on or after `date`, used for a newly-created reminder.
    pub fn on_or_after(self, date: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Daily => Some(date),
            Self::Weekly(day) if date.weekday() == day => Some(date),
            Self::Monthly(Some(day)) if Self::clamped(date.year(), date.month(), day) == Some(date) => Some(date),
            Self::Monthly(None) if Self::clamped(date.year(), date.month(), 31) == Some(date) => Some(date),
            Self::Yearly(month, day) if Self::clamped(date.year(), month, day) == Some(date) => Some(date),
            _ => self.next_after(date.pred_opt()?),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Repeat {
    pub months: Option<u32>,
    pub counter: Option<i64>,
}

/// How often a reading reminder wants a new reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Every {
    Week(u32),
    Month(u32),
    Calendar(CalendarSchedule),
}

impl Every {
    /// The interval stored as `every_n` + `every_unit`, or None when either is missing or out of
    /// range -- a row that fails this is treated as never due rather than guessed at.
    pub fn from_parts(n: Option<i64>, unit: Option<&str>) -> Option<Every> {
        let n = u32::try_from(n?).ok().filter(|n| (1..=MAX_EVERY).contains(n))?;
        match unit? {
            "week" => Some(Every::Week(n)),
            "month" => Some(Every::Month(n)),
            _ => None,
        }
    }

    /// `date` plus the interval. Calendar months clamp to the end of a shorter month, so a
    /// reading on 31 January wants the next one on 28 February, not in March.
    pub fn after(self, date: NaiveDate) -> Option<NaiveDate> {
        match self {
            Every::Week(n) => date.checked_add_days(Days::new(7 * u64::from(n))),
            Every::Month(n) => date.checked_add_months(Months::new(n)),
            Every::Calendar(schedule) => schedule.next_after(date),
        }
    }
}

/// When a reading reminder next wants a reading: one interval after the latest reading, but
/// never before the reminder's own start. With no reading at all, the start is the due date.
///
/// Derived from the data every time rather than stored: a reading logged anywhere -- a fuel
/// entry, a service, the quick reading form, a device syncing an entry made offline -- moves
/// it, and deleting that reading moves it back, with nothing to keep in step.
pub fn reading_next_due(start: NaiveDate, last_reading: Option<NaiveDate>, every: Every) -> NaiveDate {
    if let Every::Calendar(schedule) = every {
        let first = schedule.on_or_after(start).unwrap_or(start);
        // A reading before the start cannot satisfy the first occurrence. Once started,
        // an early reading satisfies the upcoming occurrence, just like early service completion.
        return match last_reading.filter(|d| *d >= start) {
            Some(last) => schedule.next_after(last.max(first)).unwrap_or(first),
            None => first,
        };
    }
    match last_reading.and_then(|d| every.after(d)) {
        Some(next) if next > start => next,
        _ => start,
    }
}

/// Whether a reading reminder is due, and the date it next wants a reading. `None` for the date
/// when the row's start or interval is unusable, which is also never due.
///
/// The one copy of this rule: `api::reminders` answers each reminder's `due` with it, and
/// `api::objects` counts an object's due readings with it.
pub fn reading_status(
    today: NaiveDate,
    start: Option<NaiveDate>,
    last_reading: Option<NaiveDate>,
    every: Option<Every>,
    snoozed_until: Option<NaiveDate>,
) -> (bool, Option<NaiveDate>) {
    let Some(next) = start.zip(every).map(|(s, e)| reading_next_due(s, last_reading, e)) else {
        return (false, None);
    };
    (is_due(today, None, Some(next), None, snoozed_until), Some(next))
}

/// Due when the date has arrived or the counter has been reached (whichever is set), unless a
/// snooze is still in effect. A snooze suppresses -- it never rewrites `due_date` or
/// `due_counter` -- so it can hide a counter-based reminder too, which moving the date never
/// could. `snoozed_until` strictly after `today` wins over both conditions; on or before today
/// the snooze has lapsed and normal rules resume.
pub fn is_due(
    today: NaiveDate,
    current_counter: Option<i64>,
    due_date: Option<NaiveDate>,
    due_counter: Option<i64>,
    snoozed_until: Option<NaiveDate>,
) -> bool {
    if snoozed_until.is_some_and(|s| s > today) {
        return false;
    }
    let by_date = due_date.map(|d| d <= today).unwrap_or(false);
    let by_counter = match (due_counter, current_counter) {
        (Some(due), Some(cur)) => cur >= due,
        _ => false,
    };
    by_date || by_counter
}

/// Whether a reminder belongs in a due-or-upcoming lookahead: already due, or coming due
/// within `within_days` (strictly in the future -- a reminder due today or in the past is
/// reported through `due`, not through the lookahead window).
///
/// A live snooze suppresses the lookahead arm as well. `due` already accounts for the snooze
/// (see `is_due`), but `days_until` is measured against the REAL due date and stays truthful
/// about it, so without this check a reminder due next week and snoozed today keeps sitting on
/// the dashboard under "upcoming" -- the one place the user just asked it not to be. Suppressed
/// only while the snooze is live; once `snoozed_until` reaches today the normal rules resume,
/// exactly as in `is_due`.
pub fn is_upcoming(
    today: NaiveDate,
    due: bool,
    days_until: Option<i64>,
    within_days: i64,
    snoozed_until: Option<NaiveDate>,
) -> bool {
    if snoozed_until.is_some_and(|s| s > today) {
        return false;
    }
    due || matches!(days_until, Some(d) if d > 0 && d <= within_days)
}

/// The (due_date, due_counter) of the follow-up reminder, or None when nothing repeats.
pub fn next_due(base_date: NaiveDate, base_counter: Option<i64>, due_counter: Option<i64>, repeat: Repeat) -> Option<(Option<NaiveDate>, Option<i64>)> {
    let date = repeat.months.and_then(|m| base_date.checked_add_months(Months::new(m)));
    let counter = repeat.counter.and_then(|step| base_counter.or(due_counter).map(|c| c + step));
    if date.is_none() && counter.is_none() { None } else { Some((date, counter)) }
}

/// Where a snoozed reminder lands: `days` after the later of today and its current due date.
pub fn snoozed_date(today: NaiveDate, current: Option<NaiveDate>, days: i64) -> NaiveDate {
    let base = match current {
        Some(d) if d > today => d,
        _ => today,
    };
    base.checked_add_days(Days::new(days.max(0) as u64)).unwrap_or(base)
}

/// Days from today until `due`; negative when it has passed. None when there is no date.
pub fn days_until(today: NaiveDate, due: Option<NaiveDate>) -> Option<i64> {
    due.map(|d| (d - today).num_days())
}

/// Counter units still to go before `due`; negative when passed. None without both readings.
pub fn counter_until(current: Option<i64>, due: Option<i64>) -> Option<i64> {
    current.zip(due).map(|(c, d)| d - c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate { NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap() }

    #[test]
    fn due_by_date() {
        assert!(is_due(d("2026-09-04"), None, Some(d("2026-09-04")), None, None));
        assert!(is_due(d("2026-09-04"), None, Some(d("2026-01-01")), None, None));
        assert!(!is_due(d("2026-09-04"), None, Some(d("2026-09-05")), None, None));
    }

    #[test]
    fn due_by_counter_needs_a_reading() {
        assert!(is_due(d("2026-09-04"), Some(10_000), None, Some(10_000), None));
        assert!(!is_due(d("2026-09-04"), Some(9_999), None, Some(10_000), None));
        assert!(!is_due(d("2026-09-04"), None, None, Some(10_000), None));
    }

    #[test]
    fn either_condition_suffices() {
        assert!(is_due(d("2026-09-04"), Some(0), Some(d("2020-01-01")), Some(10_000), None));
        assert!(is_due(d("2020-01-01"), Some(20_000), Some(d("2030-01-01")), Some(10_000), None));
    }

    #[test]
    fn a_future_snooze_suppresses_a_date_due_reminder() {
        assert!(!is_due(d("2026-09-06"), None, Some(d("2020-01-01")), None, Some(d("2026-09-13"))));
    }

    #[test]
    fn a_future_snooze_suppresses_a_counter_due_reminder() {
        // This is exactly the case a rewritten due_date could never suppress.
        assert!(!is_due(d("2026-09-06"), Some(10_000), None, Some(10_000), Some(d("2026-09-13"))));
    }

    #[test]
    fn a_lapsed_snooze_suppresses_nothing() {
        // "Today" and "in the past" both count as lapsed.
        assert!(is_due(d("2026-09-06"), Some(10_000), None, Some(10_000), Some(d("2026-09-06"))));
        assert!(is_due(d("2026-09-06"), Some(10_000), None, Some(10_000), Some(d("2020-01-01"))));
        assert!(is_due(d("2026-09-06"), None, Some(d("2020-01-01")), None, Some(d("2026-09-06"))));
    }

    #[test]
    fn next_due_from_completion_point() {
        let r = Repeat { months: Some(12), counter: Some(10_000) };
        assert_eq!(next_due(d("2026-09-04"), Some(52_000), Some(50_000), r), Some((Some(d("2027-09-04")), Some(62_000))));
        let r = Repeat { months: Some(6), counter: None };
        assert_eq!(next_due(d("2026-08-31"), None, None, r), Some((Some(d("2027-02-28")), None)));
        let r = Repeat { months: None, counter: Some(5_000) };
        assert_eq!(next_due(d("2026-09-04"), None, Some(50_000), r), Some((None, Some(55_000))));
    }

    #[test]
    fn no_repeat_means_no_follow_up() {
        assert_eq!(next_due(d("2026-09-04"), Some(1), Some(1), Repeat::default()), None);
        assert_eq!(next_due(d("2026-09-04"), None, None, Repeat { months: None, counter: Some(10) }), None);
    }

    #[test]
    fn snooze_runs_from_today_when_the_reminder_is_overdue() {
        // Three months overdue plus seven days would still be in the past, which is not
        // what pressing snooze means.
        let overdue = Some(d("2026-06-01"));
        assert_eq!(snoozed_date(d("2026-09-06"), overdue, 7), d("2026-09-13"));
    }

    #[test]
    fn snooze_runs_from_the_due_date_when_it_is_still_ahead() {
        let future = Some(d("2026-10-01"));
        assert_eq!(snoozed_date(d("2026-09-06"), future, 7), d("2026-10-08"));
    }

    #[test]
    fn snoozing_a_counter_only_reminder_gives_it_a_date() {
        assert_eq!(snoozed_date(d("2026-09-06"), None, 7), d("2026-09-13"));
    }

    #[test]
    fn days_until_has_no_value_without_a_due_date() {
        // The dashboard renders `Some(0)` as "due today" -- a counter-only reminder must not
        // get that label just because it has no date at all.
        assert_eq!(days_until(d("2026-09-06"), None), None);
    }

    #[test]
    fn days_until_counts_forward_and_backward() {
        assert_eq!(days_until(d("2026-09-06"), Some(d("2026-09-06"))), Some(0));
        assert_eq!(days_until(d("2026-09-06"), Some(d("2026-09-16"))), Some(10));
        assert_eq!(days_until(d("2026-09-06"), Some(d("2026-08-27"))), Some(-10));
    }

    #[test]
    fn counter_until_needs_both_readings() {
        assert_eq!(counter_until(None, Some(10_000)), None);
        assert_eq!(counter_until(Some(9_000), None), None);
        assert_eq!(counter_until(None, None), None);
    }

    #[test]
    fn counter_until_counts_forward_and_backward() {
        assert_eq!(counter_until(Some(9_000), Some(10_000)), Some(1_000));
        assert_eq!(counter_until(Some(10_500), Some(10_000)), Some(-500));
    }

    #[test]
    fn is_upcoming_includes_the_due_flag_regardless_of_days() {
        assert!(is_upcoming(d("2026-09-06"), true, None, 30, None));
        assert!(is_upcoming(d("2026-09-06"), true, Some(-5), 30, None));
    }

    #[test]
    fn is_upcoming_boundary_is_inclusive_of_within_days() {
        assert!(is_upcoming(d("2026-09-06"), false, Some(30), 30, None), "exactly within_days away is included");
        assert!(!is_upcoming(d("2026-09-06"), false, Some(31), 30, None), "one day beyond the window is not");
    }

    #[test]
    fn is_upcoming_excludes_the_past_and_today() {
        assert!(!is_upcoming(d("2026-09-06"), false, Some(0), 30, None), "due today is reported via `due`, not lookahead");
        assert!(!is_upcoming(d("2026-09-06"), false, Some(-1), 30, None), "a past date is not upcoming");
        assert!(!is_upcoming(d("2026-09-06"), false, None, 30, None));
    }

    /// `days_until` is measured against the real due date and stays truthful about it even
    /// while the reminder is snoozed, so the lookahead has to consult the snooze itself.
    #[test]
    fn a_live_snooze_takes_a_future_reminder_out_of_the_lookahead() {
        let today = d("2026-09-06");
        assert!(is_upcoming(today, false, Some(5), 30, None), "not snoozed: in the window");
        assert!(
            !is_upcoming(today, false, Some(5), 30, Some(d("2026-09-13"))),
            "a live snooze suppresses the lookahead arm too",
        );
    }

    #[test]
    fn every_reads_only_complete_in_range_intervals() {
        assert_eq!(Every::from_parts(Some(1), Some("month")), Some(Every::Month(1)));
        assert_eq!(Every::from_parts(Some(2), Some("week")), Some(Every::Week(2)));
        assert_eq!(Every::from_parts(Some(0), Some("month")), None);
        assert_eq!(Every::from_parts(Some(61), Some("month")), None);
        assert_eq!(Every::from_parts(Some(-1), Some("week")), None);
        assert_eq!(Every::from_parts(Some(1), Some("day")), None);
        assert_eq!(Every::from_parts(None, Some("month")), None);
        assert_eq!(Every::from_parts(Some(1), None), None);
    }

    #[test]
    fn a_monthly_interval_clamps_to_the_end_of_a_short_month() {
        assert_eq!(Every::Month(1).after(d("2026-01-31")), Some(d("2026-02-28")));
        assert_eq!(Every::Week(2).after(d("2026-12-25")), Some(d("2027-01-08")));
    }

    #[test]
    fn calendar_readings_respect_start_and_early_late_or_removed_readings() {
        let every = Every::Calendar(CalendarSchedule::Monthly(Some(31)));
        let start = d("2028-01-01");
        assert_eq!(reading_next_due(start, None, every), d("2028-01-31"));
        assert_eq!(reading_next_due(start, Some(d("2027-12-31")), every), d("2028-01-31"));
        assert_eq!(reading_next_due(start, Some(d("2028-01-15")), every), d("2028-02-29"));
        assert_eq!(reading_next_due(start, Some(d("2028-02-04")), every), d("2028-02-29"));
        assert_eq!(reading_next_due(start, Some(d("2028-02-29")), every), d("2028-03-31"));
        assert_eq!(reading_next_due(start, None, every), d("2028-01-31"));
    }

    #[test]
    fn fixed_calendar_schedules_cross_boundaries_and_clamp() {
        assert_eq!(CalendarSchedule::parse("daily").unwrap().next_after(d("2026-12-31")), Some(d("2027-01-01")));
        assert_eq!(CalendarSchedule::parse("weekly:1").unwrap().next_after(d("2026-09-21")), Some(d("2026-09-28")));
        assert_eq!(CalendarSchedule::parse("monthly:31").unwrap().next_after(d("2026-01-31")), Some(d("2026-02-28")));
        assert_eq!(CalendarSchedule::parse("monthly:last").unwrap().on_or_after(d("2028-02-01")), Some(d("2028-02-29")));
        assert_eq!(CalendarSchedule::parse("yearly:2:29").unwrap().next_after(d("2028-02-29")), Some(d("2029-02-28")));
    }

    #[test]
    fn malformed_calendar_schedules_are_refused() {
        for value in ["weekly:0", "weekly:8", "monthly:0", "monthly:32", "yearly:13:1", "yearly:2:32", "sometimes"] {
            assert!(CalendarSchedule::parse(value).is_none(), "{value}");
        }
    }

    #[test]
    fn without_a_reading_the_start_is_the_due_date() {
        assert_eq!(reading_next_due(d("2026-10-01"), None, Every::Month(1)), d("2026-10-01"));
    }

    #[test]
    fn a_reading_moves_the_next_one_an_interval_later() {
        assert_eq!(reading_next_due(d("2026-09-01"), Some(d("2026-09-20")), Every::Month(1)), d("2026-10-20"));
    }

    #[test]
    fn a_reading_older_than_the_start_does_not_pull_it_earlier() {
        // Readings logged long before anyone asked to be reminded must not make the brand-new
        // reminder overdue on the day it was created.
        assert_eq!(reading_next_due(d("2026-10-01"), Some(d("2025-01-01")), Every::Month(1)), d("2026-10-01"));
    }

    #[test]
    fn a_reading_reminder_is_due_once_the_interval_has_passed() {
        let every = Some(Every::Month(1));
        let start = Some(d("2026-01-01"));
        assert_eq!(reading_status(d("2026-09-13"), start, Some(d("2026-08-13")), every, None), (true, Some(d("2026-09-13"))));
        assert_eq!(reading_status(d("2026-09-12"), start, Some(d("2026-08-13")), every, None), (false, Some(d("2026-09-13"))));
    }

    #[test]
    fn a_snooze_hides_a_due_reading_reminder() {
        let (due, next) = reading_status(d("2026-09-13"), Some(d("2026-01-01")), None, Some(Every::Week(1)), Some(d("2026-09-20")));
        assert!(!due);
        assert_eq!(next, Some(d("2026-01-01")), "the real due date stays truthful while hidden");
    }

    #[test]
    fn an_unusable_reading_row_is_never_due() {
        assert_eq!(reading_status(d("2026-09-13"), None, None, Some(Every::Month(1)), None), (false, None));
        assert_eq!(reading_status(d("2026-09-13"), Some(d("2020-01-01")), None, None, None), (false, None));
    }

    /// The boundary matches `is_due`: `snoozed_until` ON today has lapsed, not still running.
    #[test]
    fn a_lapsed_snooze_suppresses_nothing_in_the_lookahead() {
        let today = d("2026-09-06");
        assert!(is_upcoming(today, false, Some(5), 30, Some(today)), "expiring today has lapsed");
        assert!(is_upcoming(today, false, Some(5), 30, Some(d("2026-09-01"))));
    }
}
