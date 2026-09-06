use chrono::{Days, Months, NaiveDate};

#[derive(Clone, Copy, Debug, Default)]
pub struct Repeat {
    pub months: Option<u32>,
    pub counter: Option<i64>,
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
pub fn is_upcoming(due: bool, days_until: Option<i64>, within_days: i64) -> bool {
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
        assert!(is_upcoming(true, None, 30));
        assert!(is_upcoming(true, Some(-5), 30));
    }

    #[test]
    fn is_upcoming_boundary_is_inclusive_of_within_days() {
        assert!(is_upcoming(false, Some(30), 30), "exactly within_days away is included");
        assert!(!is_upcoming(false, Some(31), 30), "one day beyond the window is not");
    }

    #[test]
    fn is_upcoming_excludes_the_past_and_today() {
        assert!(!is_upcoming(false, Some(0), 30), "due today is reported via `due`, not lookahead");
        assert!(!is_upcoming(false, Some(-1), 30), "a past date is not upcoming");
        assert!(!is_upcoming(false, None, 30));
    }
}
