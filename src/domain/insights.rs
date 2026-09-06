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
