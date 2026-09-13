use chrono::{Days, NaiveDate};

/// How far back the usage rate looks: recent enough to follow a change of habit (a new commute,
/// a winter the bike stays in), long enough to smooth out one long trip.
pub const RATE_WINDOW_DAYS: i64 = 180;
/// The shortest span a rate is measured over. Two readings a day apart say more about that day
/// than about how the object is used.
pub const RATE_MIN_SPAN_DAYS: i64 = 14;
/// An estimate further out than this is not a date anyone can plan around.
const MAX_ESTIMATE_DAYS: i64 = 3650;

/// One counter reading, as the rate needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub date: NaiveDate,
    pub counter: i64,
}

/// The newest reading: latest date, and on that date the highest value.
pub fn latest_reading(readings: &[Reading]) -> Option<Reading> {
    readings.iter().copied().max_by_key(|r| (r.date, r.counter))
}

/// Counter units per day, scaled by 1000, or None when the readings cannot support a rate.
///
/// Measured from the earliest reading inside the window to the latest one. When the window
/// holds too short a span -- readings only started recently, or there is one old reading and one
/// new -- it falls back to the earliest reading of all. A span under `RATE_MIN_SPAN_DAYS`, or a
/// counter that did not rise (a replaced odometer), gives no rate rather than a wrong one.
pub fn daily_rate_milli(readings: &[Reading]) -> Option<i64> {
    let last = latest_reading(readings)?;
    let rate_from = |first: Reading| {
        let span = (last.date - first.date).num_days();
        let delta = last.counter - first.counter;
        (span >= RATE_MIN_SPAN_DAYS && delta > 0).then(|| delta * 1000 / span)
    };
    let window_start = last.date - chrono::Duration::days(RATE_WINDOW_DAYS);
    let in_window = readings.iter().copied()
        .filter(|r| r.date >= window_start && r.date < last.date)
        .min_by_key(|r| (r.date, r.counter));
    let earliest = readings.iter().copied().min_by_key(|r| (r.date, r.counter));
    in_window.and_then(rate_from).or_else(|| earliest.and_then(rate_from))
}

/// The date the counter is expected to reach `target`, projected from the latest reading at
/// `rate_milli` units per day (scaled by 1000). None when the target is already reached -- that
/// reminder is due, not upcoming -- or when there is no usable rate.
pub fn estimated_date(last: Reading, rate_milli: i64, target: i64) -> Option<NaiveDate> {
    let remaining = target - last.counter;
    if rate_milli <= 0 || remaining <= 0 {
        return None;
    }
    let days = (remaining.saturating_mul(1000) + rate_milli - 1) / rate_milli;
    if days > MAX_ESTIMATE_DAYS {
        return None;
    }
    last.date.checked_add_days(Days::new(days as u64))
}

/// One fuel entry that carries an odometer reading, an amount, and what it cost.
#[derive(Clone, Copy, Debug)]
pub struct Fill {
    pub counter: i64,
    pub quantity_milli: i64,
    pub cost_cents: Option<i64>,
}

/// Quantity burned per 100 counter units, scaled by 1000, or None when it cannot be measured.
///
/// The standard tank method: the earliest fill only marks where the window opens -- its fuel
/// was burned before it -- so every later fill's quantity is divided by the distance from the
/// first fill to the last.
pub fn consumption_per_100_milli(fills: &[Fill]) -> Option<i64> {
    if fills.len() < 2 { return None; }
    let mut sorted = fills.to_vec();
    sorted.sort_by_key(|f| f.counter);
    let span = sorted.last()?.counter - sorted.first()?.counter;
    if span <= 0 { return None; }
    let burned: i64 = sorted[1..].iter().map(|f| f.quantity_milli).sum();
    Some(burned * 100 / span)
}

/// Cents per counter unit for the fuel window, scaled by 1000, or None when consumption
/// itself is not measurable.
///
/// The fuel block describes exactly one window: the fills that drive `consumption_per_100_milli`.
/// So this sums the cost of the same fills used there -- both a counter and a quantity, the
/// earliest one excluded -- over that same fuel span, rather than the object's overall cost
/// and counter span. A fill with no recorded cost contributes 0 but still counts as a fill, and
/// still pushes the earliest-fill-exclusion and span math the same way consumption does.
pub fn fuel_cost_per_counter_milli(fills: &[Fill]) -> Option<i64> {
    if fills.len() < 2 { return None; }
    let mut sorted = fills.to_vec();
    sorted.sort_by_key(|f| f.counter);
    let span = sorted.last()?.counter - sorted.first()?.counter;
    if span <= 0 { return None; }
    let cost: i64 = sorted[1..].iter().map(|f| f.cost_cents.unwrap_or(0)).sum();
    Some(cost * 1000 / span)
}

/// Cents per counter unit, scaled by 1000, or None when the object has not moved.
pub fn cost_per_counter_milli(total_cost_cents: i64, span: i64) -> Option<i64> {
    if span <= 0 { return None; }
    Some(total_cost_cents * 1000 / span)
}

/// The unit a quantity is in when the object does not name one: petrol countries measure
/// kilometres in litres and miles in gallons.
pub fn default_fuel_unit(counter_unit: Option<&str>) -> &'static str {
    match counter_unit {
        Some("mi") => "gal",
        _ => "l",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(counter: i64, quantity_milli: i64, cost_cents: Option<i64>) -> Fill {
        Fill { counter, quantity_milli, cost_cents }
    }

    fn day(s: &str) -> NaiveDate { NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap() }
    fn r(date: &str, counter: i64) -> Reading { Reading { date: day(date), counter } }

    #[test]
    fn the_rate_runs_from_the_earliest_reading_in_the_window_to_the_latest() {
        // 3_000 km over 100 days: 30 km a day. The 2024 reading is outside the window and must
        // not dilute the recent rate.
        let readings = [r("2024-01-01", 0), r("2026-06-01", 50_000), r("2026-09-09", 52_970)];
        assert_eq!(daily_rate_milli(&readings), Some(2_970 * 1000 / 100));
    }

    #[test]
    fn a_short_window_falls_back_to_the_earliest_reading() {
        // The only reading inside the window is five days old: too short, so the rate is taken
        // from the start of the history instead.
        let readings = [r("2025-09-09", 40_000), r("2026-09-04", 49_950), r("2026-09-09", 50_000)];
        assert_eq!(daily_rate_milli(&readings), Some(10_000 * 1000 / 365));
    }

    #[test]
    fn no_rate_without_a_real_span_or_a_rising_counter() {
        assert_eq!(daily_rate_milli(&[]), None);
        assert_eq!(daily_rate_milli(&[r("2026-09-01", 1_000)]), None);
        assert_eq!(daily_rate_milli(&[r("2026-09-01", 1_000), r("2026-09-10", 1_500)]), None, "under 14 days");
        assert_eq!(daily_rate_milli(&[r("2026-01-01", 90_000), r("2026-09-01", 1_000)]), None, "odometer replaced");
    }

    #[test]
    fn the_estimate_projects_from_the_latest_reading() {
        // 1_000 km to go at 25 km a day: 40 days after the reading, not after today.
        assert_eq!(estimated_date(r("2026-09-01", 59_000), 25_000, 60_000), Some(day("2026-10-11")));
        // A remainder that does not divide evenly rounds up: the target is reached on day 41.
        assert_eq!(estimated_date(r("2026-09-01", 59_000), 24_900, 60_000), Some(day("2026-10-12")));
    }

    #[test]
    fn no_estimate_once_reached_or_without_a_rate_or_absurdly_far() {
        assert_eq!(estimated_date(r("2026-09-01", 60_000), 25_000, 60_000), None);
        assert_eq!(estimated_date(r("2026-09-01", 59_000), 0, 60_000), None);
        assert_eq!(estimated_date(r("2026-09-01", 0), 1, 1_000_000), None);
    }

    #[test]
    fn consumption_excludes_the_first_fill() {
        // 40 L burned over 800 km -> 5 L/100 km. The first fill's fuel was burned before
        // the window opened, so only its odometer reading counts, not its litres.
        let fills = [f(10_000, 45_000, Some(5_000)), f(10_400, 20_000, Some(4_000)), f(10_800, 20_000, Some(4_000))];
        assert_eq!(consumption_per_100_milli(&fills), Some(5_000));
    }

    #[test]
    fn a_single_fill_cannot_produce_consumption() {
        assert_eq!(consumption_per_100_milli(&[f(10_000, 45_000, Some(5_000))]), None);
        assert_eq!(consumption_per_100_milli(&[]), None);
    }

    #[test]
    fn a_zero_span_produces_nothing_rather_than_dividing_by_zero() {
        assert_eq!(consumption_per_100_milli(&[f(10_000, 45_000, Some(5_000)), f(10_000, 20_000, Some(4_000))]), None);
    }

    #[test]
    fn fills_out_of_order_are_sorted_before_measuring() {
        let fills = [f(10_800, 20_000, Some(4_000)), f(10_000, 45_000, Some(5_000)), f(10_400, 20_000, Some(4_000))];
        assert_eq!(consumption_per_100_milli(&fills), Some(5_000));
    }

    #[test]
    fn cost_per_counter_needs_a_span() {
        assert_eq!(cost_per_counter_milli(48_000, 19_230), Some(2_496));
        assert_eq!(cost_per_counter_milli(48_000, 0), None);
        assert_eq!(cost_per_counter_milli(48_000, -5), None);
    }

    #[test]
    fn fuel_cost_matches_the_worked_consumption_case() {
        // Same three fills as consumption_excludes_the_first_fill: the first fill's 5_000
        // cents mark where the window opens but are not this object's to spend against it,
        // so only the later two fills' 4_000 + 4_000 = 8_000 cents count, over the 800 km
        // fuel span (not the object's overall span).
        let fills = [f(10_000, 45_000, Some(5_000)), f(10_400, 20_000, Some(4_000)), f(10_800, 20_000, Some(4_000))];
        assert_eq!(fuel_cost_per_counter_milli(&fills), Some(8_000 * 1000 / 800));
    }

    #[test]
    fn the_first_fills_cost_is_excluded_like_its_quantity() {
        // If the earliest fill's cost were folded in, this would be (9_000+4_000+4_000)*1000/800
        // = 21_250 instead of the correct (4_000+4_000)*1000/800 = 10_000.
        let fills = [f(10_000, 45_000, Some(9_000)), f(10_400, 20_000, Some(4_000)), f(10_800, 20_000, Some(4_000))];
        assert_eq!(fuel_cost_per_counter_milli(&fills), Some(10_000));
        assert_ne!(fuel_cost_per_counter_milli(&fills), Some(21_250));
    }

    #[test]
    fn a_fill_with_no_recorded_cost_contributes_zero_but_still_counts() {
        let fills = [f(10_000, 45_000, Some(5_000)), f(10_400, 20_000, None), f(10_800, 20_000, Some(4_000))];
        // Only the second and third fills count (first excluded): 0 + 4_000 over 800.
        assert_eq!(fuel_cost_per_counter_milli(&fills), Some(4_000 * 1000 / 800));
    }

    #[test]
    fn unmeasurable_consumption_means_unmeasurable_cost_too() {
        assert_eq!(fuel_cost_per_counter_milli(&[f(10_000, 45_000, Some(5_000))]), None);
        assert_eq!(fuel_cost_per_counter_milli(&[]), None);
        assert_eq!(
            fuel_cost_per_counter_milli(&[f(10_000, 45_000, Some(5_000)), f(10_000, 20_000, Some(4_000))]),
            None
        );
    }

    #[test]
    fn default_fuel_unit_uses_gallons_for_miles_and_litres_otherwise() {
        assert_eq!(default_fuel_unit(Some("mi")), "gal");
        assert_eq!(default_fuel_unit(Some("km")), "l");
        assert_eq!(default_fuel_unit(None), "l");
    }
}
