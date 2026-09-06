/// One fuel entry that carries both an odometer reading and an amount.
#[derive(Clone, Copy, Debug)]
pub struct Fill {
    pub counter: i64,
    pub quantity_milli: i64,
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

/// Cents per counter unit, scaled by 1000, or None when the object has not moved.
pub fn cost_per_counter_milli(total_cost_cents: i64, span: i64) -> Option<i64> {
    if span <= 0 { return None; }
    Some(total_cost_cents * 1000 / span)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(counter: i64, quantity_milli: i64) -> Fill { Fill { counter, quantity_milli } }

    #[test]
    fn consumption_excludes_the_first_fill() {
        // 40 L burned over 800 km -> 5 L/100 km. The first fill's fuel was burned before
        // the window opened, so only its odometer reading counts, not its litres.
        let fills = [f(10_000, 45_000), f(10_400, 20_000), f(10_800, 20_000)];
        assert_eq!(consumption_per_100_milli(&fills), Some(5_000));
    }

    #[test]
    fn a_single_fill_cannot_produce_consumption() {
        assert_eq!(consumption_per_100_milli(&[f(10_000, 45_000)]), None);
        assert_eq!(consumption_per_100_milli(&[]), None);
    }

    #[test]
    fn a_zero_span_produces_nothing_rather_than_dividing_by_zero() {
        assert_eq!(consumption_per_100_milli(&[f(10_000, 45_000), f(10_000, 20_000)]), None);
    }

    #[test]
    fn fills_out_of_order_are_sorted_before_measuring() {
        let fills = [f(10_800, 20_000), f(10_000, 45_000), f(10_400, 20_000)];
        assert_eq!(consumption_per_100_milli(&fills), Some(5_000));
    }

    #[test]
    fn cost_per_counter_needs_a_span() {
        assert_eq!(cost_per_counter_milli(48_000, 19_230), Some(2_496));
        assert_eq!(cost_per_counter_milli(48_000, 0), None);
        assert_eq!(cost_per_counter_milli(48_000, -5), None);
    }
}
