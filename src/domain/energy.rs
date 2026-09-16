//! Distance and cost per charge, and when to charge next -- pure maths over a flat list of
//! charge and trip rows, shared by `GET /objects/{id}/energy`. See
//! `docs/superpowers/specs/2026-09-16-charging-energy-design.md`.
//!
//! A charge is a `fuel` activity that carries a `counter_value`; a trip's distance is
//! `counter_value - start_counter`, exactly as `domain::trips::TripRow` computes it -- neither
//! is stored, only ever derived from the rows that hold them.

use serde::Serialize;

/// One charge (or fill), as the maths needs it. `quantity_milli` is milli-units (kWh, l, gal
/// x1000); `cost_cents` is cents. Both are optional -- a charge can mark distance alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Charge {
    /// `YYYY-MM-DD`. Charges are ordered by this, not by `counter`: a replaced or reset counter
    /// (a new odometer, a swapped battery) must not be read as one huge -- or negative -- window,
    /// and the calendar order is the only thing that can tell a replacement from ordinary use.
    pub date: String,
    pub counter: i64,
    pub quantity_milli: Option<i64>,
    pub cost_cents: Option<i64>,
    pub full: bool,
}

/// One trip, as the maths needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trip {
    pub date: String,
    /// The activity's own `start_counter`, kept alongside `distance` (not folded away) so
    /// `battery` can tell a trip that started on the charging day itself, at or after the
    /// charge's own counter, from one that finished before the charge ever happened.
    pub start_counter: i64,
    pub distance: i64,
    pub battery_used_pct: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Battery {
    pub remaining_pct: i64,
    pub range_left: Option<i64>,
    pub warn: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Energy {
    /// Mean window distance, over windows -- see `energy`'s doc comment for what a window is.
    /// `None` with no windows at all (a single window still yields a figure).
    pub distance_per_charge: Option<i64>,
    /// Mean of each window's distance per unit, scaled by 1000 (50 distance/unit -> 50_000) --
    /// the same milli scale as `quantity_milli` itself. `None` when no window's closing charge
    /// carries a (non-zero) amount.
    pub distance_per_unit_milli: Option<i64>,
    /// Mean of each window's cost per counter unit, scaled by 1000 -- the same "cents x1000"
    /// scale as `domain::insights::cost_per_counter_milli`. `None` when no window's cost can be
    /// known (see `energy`'s doc comment on the fallback through `price_milli`).
    pub cost_per_counter_milli: Option<i64>,
    /// `None` with no full charge yet, or when no trip anywhere carries a battery percentage.
    pub battery: Option<Battery>,
}

/// The arithmetic mean of `values`, rounded down, or `None` when there is nothing to average --
/// the same "no rows, no figure" rule every other domain module uses rather than reporting a
/// misleading 0.
fn mean(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<i64>() / values.len() as i64)
    }
}

/// Distance and cost per charge, and when to charge next.
///
/// Charges are ordered by date, ties broken by counter -- see `Charge::date`'s doc comment, and
/// the same rule `domain::insights::consumption_per_fill` uses for the same reason. A window is
/// the distance between two consecutive charges that "close" it: when at least two charges are
/// `full`, only consecutive **full** charges form windows (a non-full charge in between opens no
/// window of its own, so older data before charging discipline started still yields nothing
/// misleading); with fewer than two full charges, every consecutive pair forms a window instead
/// -- so the first full charge ever logged does not blank the whole section until a second one
/// arrives. A window whose distance is not positive -- a duplicate counter reading, or one that
/// went backwards (a replaced odometer, a reset battery) -- is dropped before any figure is
/// computed from it, once, so `distance_per_charge` can never disagree with the rate figures
/// about which windows exist. No windows at all leaves every figure but `battery` as `None` (a
/// single surviving window still yields one).
///
/// Each figure is the mean of its own per-window rate, not one rate over the combined windows,
/// so a single window missing an amount or a cost only drops out of the average that needs it
/// -- `distance_per_charge` still counts every window's distance.
pub fn energy(charges: &[Charge], trips: &[Trip], price_milli: Option<i64>) -> Energy {
    let mut sorted: Vec<&Charge> = charges.iter().collect();
    sorted.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.counter.cmp(&b.counter)));

    let full: Vec<&Charge> = sorted.iter().copied().filter(|c| c.full).collect();
    let pairs: Vec<(&Charge, &Charge)> = if full.len() >= 2 {
        full.windows(2).map(|w| (w[0], w[1])).collect()
    } else {
        sorted.windows(2).map(|w| (w[0], w[1])).collect()
    };
    // Filtered once, ahead of every figure below, so a duplicate or backwards counter reading is
    // simply not a window -- not a window that happens to contribute a 0 or a wildly wrong rate.
    let windows: Vec<(&Charge, &Charge)> = pairs.into_iter().filter(|(a, b)| b.counter > a.counter).collect();

    let distances: Vec<i64> = windows.iter().map(|(a, b)| b.counter - a.counter).collect();
    let distance_per_charge = mean(&distances);

    let unit_rates: Vec<i64> = windows
        .iter()
        .filter_map(|(a, b)| {
            let distance = b.counter - a.counter;
            let quantity = b.quantity_milli.filter(|&q| q != 0)?;
            // `quantity` is milli-units (a real quantity x1000), and the result is the real rate
            // ALSO scaled by 1000 -- distance / (quantity/1000) x1000 = distance x1_000_000 /
            // quantity: 400 km / 8.000 units -> 400*1_000_000/8000 = 50_000 (50 km/unit, x1000).
            // (An earlier version divided by only 1000, which cancelled `quantity_milli`'s own
            // x1000 instead of compounding it, landing 1000x too small -- e.g. reading 0.1
            // km/kWh for a rate that is really 50 times higher.)
            //
            // `i128`: `counter_value` is only validated `>= 0`, not bounded above, so
            // `distance * 1_000_000` could in principle overflow `i64` for a fat-fingered import
            // -- widen for the multiply and narrow back once the division has brought the
            // result down to a sane range, rather than risk a panic in a debug build.
            Some((distance as i128 * 1_000_000 / quantity as i128) as i64)
        })
        .collect();
    let distance_per_unit_milli = mean(&unit_rates);

    let cost_rates: Vec<i64> = windows
        .iter()
        .filter_map(|(a, b)| {
            let distance = b.counter - a.counter;
            let rate = match b.cost_cents {
                // cost (cents) x1000 / distance -- the same "cents x1000 per counter unit" scale
                // as `domain::insights::cost_per_counter_milli`.
                Some(cost) => cost * 1000 / distance,
                // No recorded cost: derive one from the price, in one division rather than
                // rounding to whole cents first and rescaling. `quantity_milli` (milli-units) x
                // `price_milli` (cents x1000 per unit) is cents x1000 x1000, so dividing by 1000
                // once lands on cents x1000 -- the same numerator the `cost_cents` branch above
                // reaches directly -- before it is divided by distance; only a single truncation
                // happens, not one at the cents step and another at the rate step.
                None => {
                    let quantity = b.quantity_milli?;
                    let price = price_milli?;
                    // `i128` for the same reason as the unit-rate multiply above: `quantity` and
                    // `price` are each only validated `>= 0`, and their product alone (before
                    // this is even divided down) could overflow `i64`.
                    (quantity as i128 * price as i128 / 1000 / distance as i128) as i64
                },
            };
            Some(rate)
        })
        .collect();
    let cost_per_counter_milli = mean(&cost_rates);

    Energy { distance_per_charge, distance_per_unit_milli, cost_per_counter_milli, battery: battery(charges, trips) }
}

/// "Charge due": how much battery is likely left, and how far that goes.
///
/// `None` when there is no full charge yet to measure from, or when no trip anywhere carries a
/// battery percentage -- neither `used` nor `km_per_pct` means anything without one.
fn battery(charges: &[Charge], trips: &[Trip]) -> Option<Battery> {
    // The full charge with the latest date; ties broken by counter, the same tie-break used for
    // ordering charges above -- a date alone does not order same-day charges.
    let last_full = charges.iter().filter(|c| c.full).max_by_key(|c| (c.date.clone(), c.counter))?;

    if !trips.iter().any(|t| t.battery_used_pct.is_some()) {
        return None;
    }

    // A trip counts once it happens on or after the charge: strictly later by date, or the same
    // day starting at or after the charge's own counter. A same-day trip that started below the
    // charge's counter ran before the charge did and has drawn on nothing from it.
    let after_last_full = |t: &&Trip| {
        t.date > last_full.date || (t.date == last_full.date && t.start_counter >= last_full.counter)
    };
    let used: i64 = trips.iter().filter(after_last_full).filter_map(|t| t.battery_used_pct).sum();
    let remaining_pct = (100 - used).max(0);

    // km_per_pct is measured over every trip that carries a battery figure, not only the ones
    // since the last full charge -- a handful of recent trips is too thin a sample for a rate,
    // and the spec asks for "trips that carry both" without a date restriction.
    let (distance_sum, pct_sum) = trips
        .iter()
        .filter_map(|t| t.battery_used_pct.map(|pct| (t.distance, pct)))
        .fold((0i64, 0i64), |(ds, ps), (d, p)| (ds + d, ps + p));
    // `remaining_pct * distance_sum / pct_sum` rather than computing `distance_sum / pct_sum`
    // first and multiplying after: one division instead of two keeps the rounding error to a
    // single truncation. `pct_sum` guards a 0 -- every trip with a battery figure recorded 0% --
    // rather than dividing by it.
    let range_left = (pct_sum > 0).then(|| remaining_pct * distance_sum / pct_sum);

    // "charge soon" -- the spec's own threshold, not configurable.
    let warn = remaining_pct <= 20;

    Some(Battery { remaining_pct, range_left, warn })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(date: &str, counter: i64, quantity_milli: Option<i64>, cost_cents: Option<i64>, full: bool) -> Charge {
        Charge { date: date.into(), counter, quantity_milli, cost_cents, full }
    }

    fn t(date: &str, start_counter: i64, distance: i64, battery_used_pct: Option<i64>) -> Trip {
        Trip { date: date.into(), start_counter, distance, battery_used_pct }
    }

    #[test]
    fn full_charges_form_windows_from_one_full_charge_to_the_next() {
        let charges = [
            c("2026-09-01", 1000, None, None, true),
            c("2026-09-05", 1400, Some(8000), Some(240), true),
            c("2026-09-10", 1800, Some(8000), Some(260), true),
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.distance_per_charge, Some(400));
        // Mean of 400*1_000_000/8000 twice: 50_000 and 50_000.
        assert_eq!(e.distance_per_unit_milli, Some(50_000));
        // Mean of 240*1000/400 (600) and 260*1000/400 (650): 625.
        assert_eq!(e.cost_per_counter_milli, Some(625));
    }

    #[test]
    fn without_a_full_charge_windows_run_between_consecutive_charges() {
        // Same counters, quantities and costs as the worked example above, but no charge is
        // marked full: the windows -- and every figure -- come out identically.
        let charges = [
            c("2026-09-01", 1000, None, None, false),
            c("2026-09-05", 1400, Some(8000), Some(240), false),
            c("2026-09-10", 1800, Some(8000), Some(260), false),
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.distance_per_charge, Some(400));
        assert_eq!(e.distance_per_unit_milli, Some(50_000));
        assert_eq!(e.cost_per_counter_milli, Some(625));
    }

    #[test]
    fn fewer_than_two_full_charges_falls_back_to_consecutive_charge_windows() {
        // Only one charge is marked full: full-to-full windows cannot form (there is no second
        // full charge to span to), so this must fall back to every consecutive pair, exactly as
        // if none were full -- otherwise the very first full charge after adopting the habit
        // would blank the whole Energy section instead of still reporting a figure.
        let charges = [
            c("2026-09-01", 1000, None, None, false),
            c("2026-09-05", 1400, Some(8000), Some(240), true),
            c("2026-09-10", 1800, Some(8000), Some(260), false),
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.distance_per_charge, Some(400));
    }

    #[test]
    fn a_non_full_charge_between_two_full_charges_does_not_split_the_window() {
        let charges = [
            c("2026-09-01", 1000, None, None, true),
            // If this counted as a window boundary it would wreck every rate below.
            c("2026-09-03", 1200, Some(1), Some(999_999), false),
            c("2026-09-05", 1800, Some(8000), Some(240), true),
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.distance_per_charge, Some(800), "one window, 1000 -> 1800");
        assert_eq!(e.distance_per_unit_milli, Some(100_000), "800*1_000_000/8000, from the 1800 charge alone");
        assert_eq!(e.cost_per_counter_milli, Some(300), "240*1000/800, from the 1800 charge alone");
    }

    #[test]
    fn distance_per_charge_is_the_mean_of_unequal_windows_not_the_first_or_the_largest() {
        let charges = [
            c("2026-09-01", 1000, None, None, true),
            c("2026-09-05", 1400, None, None, true), // a 400 window
            c("2026-09-10", 2000, None, None, true), // a 600 window
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.distance_per_charge, Some(500), "(400 + 600) / 2");
    }

    #[test]
    fn charges_are_ordered_by_date_not_by_counter_so_a_replacement_does_not_invent_a_window() {
        // A counter replaced (or reset) between the second and third charge: 98_000, 98_400,
        // then 10, 400, all full and all dated in ascending order. Sorting by counter instead of
        // date would put 10 and 400 first, inventing a huge -- or, sorted the other way, a
        // negative -- window; sorting by date puts the replacement window (98_400 -> 10) where
        // it belongs, and it is then dropped for its negative distance. The replacement charge's
        // own absurd amount and cost (999_999 milli-units, 999_999_999 cents) sit on exactly the
        // two charges that window would have used, so a wrong sort would blow every figure up --
        // this asserts the figures stay the small, sane numbers the surviving windows alone give.
        let charges = [
            c("2026-01-01", 98_000, None, None, true),
            c("2026-06-01", 98_400, Some(8_000), Some(240), true),
            c("2026-09-01", 10, Some(999_999), Some(999_999_999), true),
            c("2026-09-05", 400, Some(8_000), Some(260), true),
        ];
        let e = energy(&charges, &[], None);
        // Surviving windows: 98_000 -> 98_400 (400) and 10 -> 400 (390); the 98_400 -> 10 window
        // is dropped for its negative distance. Mean: (400 + 390) / 2 = 395.
        assert_eq!(e.distance_per_charge, Some(395));
        // 400*1_000_000/8000 = 50_000 exactly; 390*1_000_000/8000 = 48_750 exactly (unlike the
        // old x1000 formula, this doesn't truncate 48.75 down to 48 first -- the extra factor
        // leaves room for the fraction). Mean: (50_000 + 48_750) / 2 = 49_375.
        assert_eq!(e.distance_per_unit_milli, Some(49_375));
        // Mean of 240*1000/400 (600) and 260*1000/390 (666): 633.
        assert_eq!(e.cost_per_counter_milli, Some(633));
    }

    #[test]
    fn a_zero_distance_window_is_dropped_rather_than_dragging_the_mean_down() {
        // Two charges logged for the same day at the same counter (a correction, or a duplicate
        // entry), then a real 400-unit window. The zero-distance window must not count as a
        // window at all: if it did, distance_per_charge would be (0 + 400) / 2 = 200 instead of
        // just 400.
        let charges = [
            c("2026-09-01", 1000, None, None, true),
            c("2026-09-01", 1000, None, None, true),
            c("2026-09-05", 1400, None, None, true),
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.distance_per_charge, Some(400));
    }

    #[test]
    fn a_closing_charge_without_an_amount_still_counts_for_distance_per_charge() {
        let charges = [
            c("2026-09-01", 1000, None, None, true),
            // Closes the first window with no quantity: it still marks 400 units of distance,
            // but that window contributes nothing to distance_per_unit_milli.
            c("2026-09-05", 1400, None, Some(240), true),
            c("2026-09-10", 1800, Some(8000), Some(260), true),
        ];
        let e = energy(&charges, &[], None);
        // Both windows (400, 400) count: mean is still 400.
        assert_eq!(e.distance_per_charge, Some(400));
        // Only the second window has an amount: 400*1_000_000/8000 = 50_000, alone -- not
        // averaged with a missing first value.
        assert_eq!(e.distance_per_unit_milli, Some(50_000));
    }

    #[test]
    fn a_quantity_of_zero_is_skipped_rather_than_dividing_by_zero() {
        let charges = [
            c("2026-09-01", 1000, None, None, true),
            c("2026-09-05", 1400, Some(0), Some(240), true),
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.distance_per_unit_milli, None);
    }

    #[test]
    fn a_closing_charge_without_a_cost_falls_back_to_price_or_is_skipped() {
        let charges = [
            c("2026-09-01", 1000, None, None, true),
            // No cost and no price: this window is skipped for cost_per_counter_milli.
            c("2026-09-05", 1400, Some(8000), None, true),
        ];
        let e = energy(&charges, &[], None);
        assert_eq!(e.cost_per_counter_milli, None);

        // A window whose numbers do not divide evenly, to prove the derived rate is one
        // truncation (quantity * price / 1000 / distance), not two (quantity * price / 1000 /
        // 1000 rounded to whole cents, then rescaled): 8_500 milli-units at 27_500 (cents x1000
        // per unit, i.e. 27.5 cents/unit) over a 333-unit window.
        // quantity * price / 1000 = 8_500 * 27_500 / 1000 = 233_750 (cents x1000); / 333 = 701
        // (truncated once). Rounding to cents first (233_750 / 1000 = 233) and then rescaling
        // (233 * 1000 / 333 = 699) would lose precision earlier and land on the wrong integer.
        let charges = [c("2026-09-01", 1000, None, None, true), c("2026-09-01", 1333, Some(8_500), None, true)];
        let e = energy(&charges, &[], Some(27_500));
        assert_eq!(e.cost_per_counter_milli, Some(701));
    }

    #[test]
    fn fewer_than_two_charges_gives_every_figure_none() {
        assert_eq!(energy(&[], &[], None).distance_per_charge, None);
        let one = [c("2026-09-01", 1000, Some(8000), Some(240), true)];
        let e = energy(&one, &[], None);
        assert_eq!(e.distance_per_charge, None);
        assert_eq!(e.distance_per_unit_milli, None);
        assert_eq!(e.cost_per_counter_milli, None);
        assert_eq!(e.battery, None);
    }

    #[test]
    fn battery_remaining_and_range_left_from_trips_after_the_last_full_charge() {
        let charges = [c("2026-09-01", 1000, None, None, true)];
        let trips = [t("2026-09-03", 0, 60, Some(30)), t("2026-09-05", 0, 50, Some(25))];
        let e = energy(&charges, &trips, None);
        // used = 30 + 25 = 55; remaining = 100 - 55 = 45.
        // km_per_pct over all trips carrying both: (60+50) / (30+25) = 110/55 = 2.
        // range_left = 45 * 2 = 90.
        assert_eq!(e.battery, Some(Battery { remaining_pct: 45, range_left: Some(90), warn: false }));
    }

    #[test]
    fn battery_warns_once_remaining_drops_to_20_or_below() {
        let charges = [c("2026-09-01", 1000, None, None, true)];
        // used = 85 -> remaining = 15 (<=20, warns). range_left = 15 * 100 / 85 = 17.
        let trips = [t("2026-09-03", 0, 100, Some(85))];
        let e = energy(&charges, &trips, None);
        assert_eq!(e.battery, Some(Battery { remaining_pct: 15, range_left: Some(17), warn: true }));

        // used = 120 -> remaining clamps to 0, not negative, and still warns.
        let trips = [t("2026-09-03", 0, 100, Some(120))];
        let e = energy(&charges, &trips, None);
        let battery = e.battery.expect("a full charge and a trip with a battery figure");
        assert_eq!(battery.remaining_pct, 0);
        assert_eq!(battery.range_left, Some(0));
        assert!(battery.warn);
    }

    #[test]
    fn a_trip_on_the_charging_day_counts_when_it_starts_at_or_after_the_charge() {
        let charges = [c("2026-09-01", 1000, None, None, true)];

        // Same day as the full charge, starting exactly at its counter: counts.
        let trips = [t("2026-09-01", 1000, 40, Some(40))];
        let e = energy(&charges, &trips, None);
        assert_eq!(e.battery.unwrap().remaining_pct, 60);

        // Same day, starting above the charge's counter: also counts.
        let trips = [t("2026-09-01", 1005, 40, Some(40))];
        let e = energy(&charges, &trips, None);
        assert_eq!(e.battery.unwrap().remaining_pct, 60);

        // Same day, but starting below the charge's counter: this trip ran before the charge
        // happened, so it must not count against it.
        let trips = [t("2026-09-01", 990, 10, Some(40))];
        let e = energy(&charges, &trips, None);
        assert_eq!(e.battery.unwrap().remaining_pct, 100);
    }

    #[test]
    fn only_trips_after_the_latest_full_charge_count_towards_used() {
        let charges = [c("2026-09-01", 1000, None, None, true), c("2026-09-10", 1800, None, None, true)];
        let trips = [
            // Between the two full charges: not after the *latest* one, so excluded from used.
            t("2026-09-05", 1000, 100, Some(50)),
            // After the latest full charge: counts.
            t("2026-09-12", 1800, 50, Some(20)),
        ];
        let e = energy(&charges, &trips, None);
        // used = 20 only; remaining = 80. (Counting both would give used = 70, remaining = 30.)
        assert_eq!(e.battery.unwrap().remaining_pct, 80);
    }

    #[test]
    fn no_full_charge_or_no_trip_with_a_battery_figure_gives_no_battery_block() {
        // A trip with a battery figure, but no full charge yet.
        let charges = [c("2026-09-01", 1000, None, None, false)];
        let trips = [t("2026-09-03", 0, 60, Some(30))];
        assert_eq!(energy(&charges, &trips, None).battery, None);

        // A full charge, but no trip carries a battery figure.
        let charges = [c("2026-09-01", 1000, None, None, true)];
        let trips = [t("2026-09-03", 0, 60, None)];
        assert_eq!(energy(&charges, &trips, None).battery, None);
    }

    #[test]
    fn a_battery_percent_of_zero_gives_no_range_left_but_still_leaves_a_battery_block() {
        let charges = [c("2026-09-01", 1000, None, None, true)];
        // The only trip with a battery figure recorded 0%: pct_sum is 0, so km_per_pct cannot be
        // measured -- this must guard the division, not perform it.
        let trips = [t("2026-09-03", 0, 100, Some(0))];
        let e = energy(&charges, &trips, None);
        assert_eq!(e.battery, Some(Battery { remaining_pct: 100, range_left: None, warn: false }));
    }
}
