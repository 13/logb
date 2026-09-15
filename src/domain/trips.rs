//! Trip totals: pure aggregation over a flat list of trip rows, shared by
//! `GET /objects/{id}/trips/summary`'s `month`/`year`/`all` periods.
//!
//! A trip's distance is `end - start` (the activity's `counter_value` minus its
//! `start_counter`); nothing here stores it, exactly as the row it aggregates never does either
//! -- see `api::activities::ActivityRow::start_counter`.

use serde::Serialize;

/// One trip, as the aggregation needs it.
pub struct TripRow {
    /// `YYYY-MM-DD`. Compared with `prefix` by `starts_with`, so `totals` needs no parsed date.
    pub date: String,
    pub start: i64,
    pub end: i64,
    pub duration_minutes: Option<i64>,
    pub battery_used_pct: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TripTotals {
    pub trips: i64,
    pub distance: i64,
    /// `distance / trips`, rounded down. `None` with no trips.
    pub avg_distance: Option<i64>,
    /// Average speed, km/h (or mi/h) ×10, over the trips that carry a duration: their combined
    /// distance ×600 (60 minutes/hour ×10) over their combined minutes. `None` when none of the
    /// matching trips has a duration.
    pub speed_x10: Option<i64>,
    /// Distance per 10% battery used, over the trips that carry a battery figure: their combined
    /// distance ×10 over their combined percentage. `None` when none of the matching trips has a
    /// battery figure, or their combined percentage is 0.
    pub distance_per_10pct: Option<i64>,
}

/// Totals for the trips whose date starts with `prefix` (`""` = all, `"2026"` = year,
/// `"2026-09"` = month).
///
/// `speed_x10` and `distance_per_10pct` are each computed over their own combined distance and
/// combined minutes/percentage -- not averaged per trip -- so a handful of short trips with a
/// duration and one long trip without one still gives an honest overall speed rather than one
/// dominated by whichever trip happens to carry the field.
pub fn totals(rows: &[TripRow], prefix: &str) -> TripTotals {
    let matching: Vec<&TripRow> = rows.iter().filter(|r| r.date.starts_with(prefix)).collect();
    let trips = matching.len() as i64;
    let distance: i64 = matching.iter().map(|r| r.end - r.start).sum();
    let avg_distance = (trips > 0).then(|| distance / trips);

    let (dur_distance, dur_minutes) = matching.iter().filter_map(|r| r.duration_minutes.map(|m| (r.end - r.start, m)))
        .fold((0i64, 0i64), |(ds, dm), (d, m)| (ds + d, dm + m));
    // Guarded even though `duration_minutes` is validated to never be 0 on a stored row
    // (`ActivityInput::validate`): this function only trusts what it is handed.
    let speed_x10 = (dur_minutes > 0).then(|| dur_distance * 600 / dur_minutes);

    let (batt_distance, batt_pct) = matching.iter().filter_map(|r| r.battery_used_pct.map(|b| (r.end - r.start, b)))
        .fold((0i64, 0i64), |(ds, bp), (d, b)| (ds + d, bp + b));
    let distance_per_10pct = (batt_pct > 0).then(|| batt_distance * 10 / batt_pct);

    TripTotals { trips, distance, avg_distance, speed_x10, distance_per_10pct }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(date: &str, start: i64, end: i64, duration_minutes: Option<i64>, battery_used_pct: Option<i64>) -> TripRow {
        TripRow { date: date.into(), start, end, duration_minutes, battery_used_pct }
    }

    #[test]
    fn no_rows_gives_zero_trips_and_every_average_null() {
        let t = totals(&[], "");
        assert_eq!(t, TripTotals { trips: 0, distance: 0, avg_distance: None, speed_x10: None, distance_per_10pct: None });
    }

    #[test]
    fn two_trips_combine_distance_speed_and_battery_range() {
        let rows = [
            row("2026-09-01", 400, 600, Some(75), Some(32)),
            row("2026-09-02", 600, 650, None, Some(10)),
        ];
        let t = totals(&rows, "");
        assert_eq!(t.trips, 2);
        assert_eq!(t.distance, 250);
        assert_eq!(t.avg_distance, Some(125));
        // Only the first trip has a duration: 200 km in 75 min -> 200*60*10/75 = 1600 (160.0 km/h x10).
        assert_eq!(t.speed_x10, Some(1600));
        // Both trips have a battery figure: (200+50)*10/(32+10) = 2500/42 = 59 (integer division).
        assert_eq!(t.distance_per_10pct, Some(59));
    }

    #[test]
    fn prefix_filters_by_date_and_avg_distance_rounds_down() {
        let rows = [
            row("2025-12-31", 0, 100, None, None),
            row("2026-03-10", 100, 210, None, None),
            row("2026-09-02", 210, 340, None, None),
        ];
        let year = totals(&rows, "2026");
        // Two trips this year: 110 + 130 = 240, 240/2 = 120 exactly.
        assert_eq!((year.trips, year.distance, year.avg_distance), (2, 240, Some(120)));

        let month = totals(&rows, "2026-09");
        assert_eq!((month.trips, month.distance, month.avg_distance), (1, 130, Some(130)));

        let all = totals(&rows, "");
        // Three trips: 100 + 110 + 130 = 340, 340/3 = 113 (rounds down, not 113.33).
        assert_eq!((all.trips, all.distance, all.avg_distance), (3, 340, Some(113)));
    }

    #[test]
    fn a_battery_sum_of_zero_gives_no_distance_per_10pct() {
        let rows = [row("2026-09-01", 0, 100, None, Some(0)), row("2026-09-02", 100, 180, None, Some(0))];
        assert_eq!(totals(&rows, "").distance_per_10pct, None);
    }

    #[test]
    fn a_zero_duration_guards_rather_than_divides_by_zero() {
        // `duration_minutes` is validated to be at least 1 on any row the API stores, but this
        // aggregation trusts nothing about its input, so a 0 is guarded like a missing sum would be.
        let rows = [row("2026-09-01", 0, 100, Some(0), None)];
        assert_eq!(totals(&rows, "").speed_x10, None);
    }
}
