use chrono::{Months, NaiveDate};

#[derive(Clone, Copy, Debug, Default)]
pub struct Repeat {
    pub months: Option<u32>,
    pub counter: Option<i64>,
}

/// Due when the date has arrived or the counter has been reached (whichever is set).
pub fn is_due(today: NaiveDate, current_counter: Option<i64>, due_date: Option<NaiveDate>, due_counter: Option<i64>) -> bool {
    let by_date = due_date.map(|d| d <= today).unwrap_or(false);
    let by_counter = match (due_counter, current_counter) {
        (Some(due), Some(cur)) => cur >= due,
        _ => false,
    };
    by_date || by_counter
}

/// The (due_date, due_counter) of the follow-up reminder, or None when nothing repeats.
pub fn next_due(base_date: NaiveDate, base_counter: Option<i64>, due_counter: Option<i64>, repeat: Repeat) -> Option<(Option<NaiveDate>, Option<i64>)> {
    let date = repeat.months.and_then(|m| base_date.checked_add_months(Months::new(m)));
    let counter = repeat.counter.and_then(|step| base_counter.or(due_counter).map(|c| c + step));
    if date.is_none() && counter.is_none() { None } else { Some((date, counter)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate { NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap() }

    #[test]
    fn due_by_date() {
        assert!(is_due(d("2026-09-04"), None, Some(d("2026-09-04")), None));
        assert!(is_due(d("2026-09-04"), None, Some(d("2026-01-01")), None));
        assert!(!is_due(d("2026-09-04"), None, Some(d("2026-09-05")), None));
    }

    #[test]
    fn due_by_counter_needs_a_reading() {
        assert!(is_due(d("2026-09-04"), Some(10_000), None, Some(10_000)));
        assert!(!is_due(d("2026-09-04"), Some(9_999), None, Some(10_000)));
        assert!(!is_due(d("2026-09-04"), None, None, Some(10_000)));
    }

    #[test]
    fn either_condition_suffices() {
        assert!(is_due(d("2026-09-04"), Some(0), Some(d("2020-01-01")), Some(10_000)));
        assert!(is_due(d("2020-01-01"), Some(20_000), Some(d("2030-01-01")), Some(10_000)));
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
}
