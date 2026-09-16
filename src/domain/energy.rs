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
    /// `YYYY-MM-DD`, compared lexically -- see `battery` below.
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
    /// `None` with fewer than two qualifying charges.
    pub distance_per_charge: Option<i64>,
    /// Mean of each window's distance per unit, scaled by 1000 (0.05 distance/unit -> 50) --
    /// the same milli scale as `quantity_milli` itself. `None` when no window's closing charge
    /// carries an amount.
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

/// A charge's cost for one window's rate: its own `cost_cents` when known, else `quantity_milli
/// x price_milli` when both exist, else unknown.
///
/// The scale conversion, spelled out because two milli scales meet here: `quantity_milli` is
/// the amount in milli-units (8.5 kWh stored as 8_500), `price_milli` is cents per unit x1000
/// (0.30 EUR/kWh stored as 30_000). Their product is cents x1000 x1000, so it is divided by
/// 1000 twice -- once for each scale factor -- to land back on plain cents: `8_000 * 30_000 /
/// 1000 / 1000 = 240` cents for 8 kWh at 30 cents/kWh.
fn known_cost_cents(charge: &Charge, price_milli: Option<i64>) -> Option<i64> {
    charge.cost_cents.or_else(|| {
        let quantity = charge.quantity_milli?;
        let price = price_milli?;
        Some(quantity * price / 1000 / 1000)
    })
}

/// Distance and cost per charge, and when to charge next.
///
/// A window is the distance between two consecutive charges that "close" it: when any charge in
/// `charges` is `full`, only consecutive **full** charges form windows (a non-full charge in
/// between opens no window of its own, so older data before charging discipline started still
/// yields nothing misleading); otherwise every consecutive pair of charges, sorted by counter,
/// forms one. Fewer than two qualifying charges gives no windows at all, and every figure but
/// `battery` is `None`.
///
/// Each figure is the mean of its own per-window rate, not one rate over the combined windows,
/// so a single window missing an amount or a cost only drops out of the average that needs it
/// -- `distance_per_charge` still counts every window's distance.
pub fn energy(charges: &[Charge], trips: &[Trip], price_milli: Option<i64>) -> Energy {
    let mut sorted: Vec<&Charge> = charges.iter().collect();
    sorted.sort_by_key(|c| c.counter);

    let full: Vec<&Charge> = sorted.iter().copied().filter(|c| c.full).collect();
    // `Vec<&Charge>::windows` needs a slice of the chosen subset, sorted the same way as `sorted`
    // already is (a filter preserves order).
    let windows: Vec<(&Charge, &Charge)> = if full.is_empty() {
        sorted.windows(2).map(|w| (w[0], w[1])).collect()
    } else {
        full.windows(2).map(|w| (w[0], w[1])).collect()
    };

    let distances: Vec<i64> = windows.iter().map(|(a, b)| b.counter - a.counter).collect();
    let distance_per_charge = mean(&distances);

    let unit_rates: Vec<i64> = windows
        .iter()
        .filter_map(|(a, b)| {
            let distance = b.counter - a.counter;
            let quantity = b.quantity_milli.filter(|&q| q != 0)?;
            // distance (counter units) x1000 / quantity (milli-units) keeps the milli scale
            // `Energy::distance_per_unit_milli` documents: 400 km / 8.000 units -> 400*1000/8000
            // = 50 (0.05 distance per unit, x1000).
            (distance > 0).then(|| distance * 1000 / quantity)
        })
        .collect();
    let distance_per_unit_milli = mean(&unit_rates);

    let cost_rates: Vec<i64> = windows
        .iter()
        .filter_map(|(a, b)| {
            let distance = b.counter - a.counter;
            let cost = known_cost_cents(b, price_milli)?;
            // cost (cents) x1000 / distance -- the same "cents x1000 per counter unit" scale as
            // `domain::insights::cost_per_counter_milli`.
            (distance > 0).then(|| cost * 1000 / distance)
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
    // The full charge with the latest date; ties broken by counter, the same tie-break
    // `domain::insights::latest_reading` uses for the same reason -- a date alone does not
    // order same-day charges.
    let last_full = charges.iter().filter(|c| c.full).max_by_key(|c| (c.date.clone(), c.counter))?;

    if !trips.iter().any(|t| t.battery_used_pct.is_some()) {
        return None;
    }

    let used: i64 = trips
        .iter()
        .filter(|t| t.date > last_full.date)
        .filter_map(|t| t.battery_used_pct)
        .sum();
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
    // single truncation.
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

    fn t(date: &str, distance: i64, battery_used_pct: Option<i64>) -> Trip {
        Trip { date: date.into(), distance, battery_used_pct }
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
        // Mean of 400*1000/8000 twice: 50 and 50.
        assert_eq!(e.distance_per_unit_milli, Some(50));
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
        assert_eq!(e.distance_per_unit_milli, Some(50));
        assert_eq!(e.cost_per_counter_milli, Some(625));
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
        // Only the second window has an amount: 400*1000/8000 = 50, alone -- not averaged with
        // a missing first value.
        assert_eq!(e.distance_per_unit_milli, Some(50));
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

        // The same charges, but with a price to derive a cost from: 8_000 milli-units at
        // 30_000 (cents x1000 per unit, i.e. 30 cents/unit) -> 8_000 * 30_000 / 1000 / 1000 =
        // 240 cents, over the 400-unit window: 240 * 1000 / 400 = 600.
        let e = energy(&charges, &[], Some(30_000));
        assert_eq!(e.cost_per_counter_milli, Some(600));
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
        let trips = [t("2026-09-03", 60, Some(30)), t("2026-09-05", 50, Some(25))];
        let e = energy(&charges, &trips, None);
        // used = 30 + 25 = 55; remaining = 100 - 55 = 45.
        // km_per_pct over all trips carrying both: (60+50) / (30+25) = 110/55 = 2.
        // range_left = 45 * 2 = 90.
        assert_eq!(e.battery, Some(Battery { remaining_pct: 45, range_left: Some(90), warn: false }));
    }

    #[test]
    fn battery_warns_once_remaining_drops_to_20_or_below() {
        let charges = [c("2026-09-01", 1000, None, None, true)];
        // used = 85 -> remaining = 15 (<=20, warns).
        let trips = [t("2026-09-03", 100, Some(85))];
        let e = energy(&charges, &trips, None);
        assert_eq!(e.battery, Some(Battery { remaining_pct: 15, range_left: Some(15 * 100 / 85), warn: true }));

        // used = 120 -> remaining clamps to 0, not negative, and still warns.
        let trips = [t("2026-09-03", 100, Some(120))];
        let e = energy(&charges, &trips, None);
        let battery = e.battery.expect("a full charge and a trip with a battery figure");
        assert_eq!(battery.remaining_pct, 0);
        assert_eq!(battery.range_left, Some(0));
        assert!(battery.warn);
    }

    #[test]
    fn no_full_charge_or_no_trip_with_a_battery_figure_gives_no_battery_block() {
        // A trip with a battery figure, but no full charge yet.
        let charges = [c("2026-09-01", 1000, None, None, false)];
        let trips = [t("2026-09-03", 60, Some(30))];
        assert_eq!(energy(&charges, &trips, None).battery, None);

        // A full charge, but no trip carries a battery figure.
        let charges = [c("2026-09-01", 1000, None, None, true)];
        let trips = [t("2026-09-03", 60, None)];
        assert_eq!(energy(&charges, &trips, None).battery, None);
    }
}
