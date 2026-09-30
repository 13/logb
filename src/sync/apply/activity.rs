//! The cross-field rules a `set` on an activity has to pass against the rest of its row.

use crate::error::AppError;
use crate::sync::{Entity, Op, Outcome};
use super::value::{Binding, TripRow};

/// Answers `Outcome::Accepted` when the activity's row stays valid with `field` set to `bound`,
/// and the rejection otherwise. Any other entity passes untouched.
pub(super) async fn check_activity(
    tx: &mut sqlx::AnyConnection,
    op: &Op,
    field: &str,
    bound: &Binding,
) -> Result<Outcome, AppError> {
    // The trip cross-field rules: something no single field's shape or range can say on
    // its own, so they are checked against the row's OTHER current values, the way
    // `ActivityInput::validate` checks them against the rest of a REST PATCH's body. A
    // REST PATCH resends every field together and is validated as one combination; a
    // synced `set` touches exactly one field, checked against what is stored RIGHT NOW
    // -- so a client meaning to change several of these together, especially `category`
    // to or from `trip`, has to either order its ops so each one lands in a state the
    // next one's check can pass, or send the whole change as one REST PATCH instead,
    // which is what a category change to or from `trip` is meant to go through (see
    // `docs/openapi.json`'s `/sync/push` description).
    //
    // This also has to sit ahead of the last-write-wins comparison below, not after:
    // an op that is invalid against the row as it stands is Rejected outright, on
    // purpose, never Superseded. Superseded is for a stale but otherwise valid value
    // losing a race it would have won moments earlier; invalid data is never stored
    // regardless of timing, so an old, invalid op must not reach the one comparison
    // that only ever decides who wins a race between two values that were each fine on
    // their own.
    if op.entity == Entity::Activity
        && matches!(
            field,
            "category"
                | "start_counter"
                | "from_place"
                | "to_place"
                | "duration_minutes"
                | "battery_used_pct"
                | "counter_value"
        )
    {
        let stored: TripRow = sqlx::query_as(
            "SELECT a.category, a.counter_value, a.start_counter, a.from_place, a.to_place, \
             a.duration_minutes, a.battery_used_pct, o.counter_unit \
             FROM activities a JOIN objects o ON o.id = a.object_id WHERE a.client_uuid = $1",
        )
        .bind(&op.entity_uuid)
        .fetch_one(&mut *tx)
        .await?;

        const ONLY_A_TRIP: &str =
            "only a trip has start_counter, places, duration or battery";
        const NEEDS_BOTH: &str = "a trip needs start_counter and counter_value";
        const START_RANGE: &str = "start_counter must be between 0 and counter_value";
        const NEEDS_KM_MI: &str = "a trip needs an object that counts km or mi";

        if field == "category" {
            // A `null` category cannot reach here as `Binding::Text`; `binding` already
            // shaped it as `Binding::Null`, and the column's own NOT NULL constraint is
            // what refuses that, exactly as it does for any other required TEXT field.
            if let Binding::Text(new_category) = &bound {
                if new_category == "trip" {
                    if !matches!(stored.counter_unit.as_deref(), Some("km") | Some("mi")) {
                        return Ok(Outcome::Rejected {
                            reason: NEEDS_KM_MI.into(),
                        });
                    }
                    match (stored.start_counter, stored.counter_value) {
                        (Some(s), Some(e)) if (0..=e).contains(&s) => {}
                        (Some(_), Some(_)) => {
                            return Ok(Outcome::Rejected {
                                reason: START_RANGE.into(),
                            })
                        }
                        _ => {
                            return Ok(Outcome::Rejected {
                                reason: NEEDS_BOTH.into(),
                            })
                        }
                    }
                } else if new_category != "session" && stored.has_trip_fields() {
                    return Ok(Outcome::Rejected {
                        reason: ONLY_A_TRIP.into(),
                    });
                }
            }
        } else if field == "counter_value" {
            // Not trip-only -- every category may carry one -- so only checked against
            // `start_counter` when the row IS a trip, and a trip may never lose it: an
            // end with no start left is exactly the row `ActivityInput::validate`
            // refuses to create in the first place.
            if stored.category == "trip" {
                match &bound {
                    Binding::Integer(n)
                        if stored.start_counter.is_some_and(|s| (0..=*n).contains(&s)) => {}
                    Binding::Integer(_) => {
                        return Ok(Outcome::Rejected {
                            reason: START_RANGE.into(),
                        })
                    }
                    _ => {
                        return Ok(Outcome::Rejected {
                            reason: NEEDS_BOTH.into(),
                        })
                    }
                }
            }
        } else if stored.category != "trip" && stored.category != "session" {
            // The four trip-only fields, plus `start_counter`: refused outright on a
            // non-trip row, unless they are being cleared -- clearing stays legal
            // regardless of category, exactly as on the REST door.
            if !matches!(&bound, Binding::Null) {
                return Ok(Outcome::Rejected {
                    reason: ONLY_A_TRIP.into(),
                });
            }
        } else if stored.category == "session" {
            if matches!(field, "start_counter" | "to_place" | "battery_used_pct")
                && !matches!(&bound, Binding::Null)
            {
                return Ok(Outcome::Rejected {
                    reason: "a session only has a place and duration".into(),
                });
            }
        } else if field == "start_counter" {
            // On a trip row specifically: a trip may never lose its start either,
            // mirroring `counter_value` above.
            match &bound {
                Binding::Integer(n)
                    if stored.counter_value.is_some_and(|e| (0..=e).contains(n)) => {}
                Binding::Integer(_) => {
                    return Ok(Outcome::Rejected {
                        reason: START_RANGE.into(),
                    })
                }
                _ => {
                    return Ok(Outcome::Rejected {
                        reason: NEEDS_BOTH.into(),
                    })
                }
            }
        }
        // The three other trip-only fields need no further check here on a trip row:
        // clearing or resetting `from_place`/`to_place`/`duration_minutes`/
        // `battery_used_pct` never makes the row invalid the way losing `start_counter`
        // or `counter_value` would.
    }

    // The charge cross-field rule, independent of the trip checks above: `charged_full`
    // may only ever sit at 1 on a `fuel` row, in either direction -- setting the flag
    // directly, or moving the row's own category away from `fuel` while the flag is
    // still stored. A separate query rather than folding into `TripRow`/its `matches!`
    // list above: `charged_full` is not one of the trip-only fields that block's
    // catch-all (`stored.category != "trip"`) exists to guard, and it needs none of
    // that block's other state.
    if op.entity == Entity::Activity
        && matches!(
            field,
            "category" | "weight_grams" | "counter_value" | "quantity_milli" | "cost_cents"
        )
    {
        let (category, weight, counter, quantity, cost): (String, Option<i64>, Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT category, weight_grams, counter_value, quantity_milli, cost_cents FROM activities WHERE client_uuid = $1")
            .bind(&op.entity_uuid).fetch_one(&mut *tx).await?;
        let value = op.value.as_ref().unwrap_or(&serde_json::Value::Null);
        let category = if field == "category" {
            value.as_str().unwrap_or("")
        } else {
            &category
        };
        let weight = if field == "weight_grams" {
            value.as_i64()
        } else {
            weight
        };
        let counter = if field == "counter_value" {
            value.as_i64()
        } else {
            counter
        };
        let quantity = if field == "quantity_milli" {
            value.as_i64()
        } else {
            quantity
        };
        let cost = if field == "cost_cents" {
            value.as_i64()
        } else {
            cost
        };
        if (category == "weight"
            && (weight.is_none()
                || counter.is_some()
                || quantity.is_some()
                || cost.is_some()))
            || (category != "weight" && weight.is_some())
        {
            return Ok(Outcome::Rejected { reason: "weight entries require weight_grams and cannot carry counters, fuel or costs".into() });
        }
    }
    if op.entity == Entity::Activity && matches!(field, "category" | "charged_full") {
        let (stored_category, stored_charged_full): (String, i64) = sqlx::query_as(
            "SELECT category, charged_full FROM activities WHERE client_uuid = $1",
        )
        .bind(&op.entity_uuid)
        .fetch_one(&mut *tx)
        .await?;
        if field == "charged_full" {
            if matches!(&bound, Binding::Integer(1))
                && !matches!(stored_category.as_str(), "fuel" | "usage")
            {
                return Ok(Outcome::Rejected {
                    reason: crate::api::activities::CHARGE_FULL_ONLY.into(),
                });
            }
        } else if let Binding::Text(new_category) = &bound {
            if !matches!(new_category.as_str(), "fuel" | "usage")
                && stored_charged_full == 1
            {
                return Ok(Outcome::Rejected {
                    reason: crate::api::activities::CHARGE_FULL_ONLY.into(),
                });
            }
        }
    }
    if op.entity == Entity::Activity
        && matches!(field, "category" | "fuel_level_pct" | "quantity_milli")
    {
        let (stored_category, stored_level, stored_quantity, fuel_unit, counter_unit): (String, Option<i64>, Option<i64>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT a.category, a.fuel_level_pct, a.quantity_milli, o.fuel_unit, o.counter_unit FROM activities a JOIN objects o ON o.id = a.object_id WHERE a.client_uuid = $1")
            .bind(&op.entity_uuid).fetch_one(&mut *tx).await?;
        let new_category = if field == "category" {
            match &bound {
                Binding::Text(v) => v.as_str(),
                _ => stored_category.as_str(),
            }
        } else {
            stored_category.as_str()
        };
        let new_level = if field == "fuel_level_pct" {
            match &bound {
                Binding::Integer(v) => Some(*v),
                Binding::Null => None,
                _ => stored_level,
            }
        } else {
            stored_level
        };
        let new_quantity = if field == "quantity_milli" {
            match &bound {
                Binding::Integer(v) => Some(*v),
                Binding::Null => None,
                _ => stored_quantity,
            }
        } else {
            stored_quantity
        };
        if new_level.is_some() && !matches!(new_category, "fuel" | "usage") {
            return Ok(Outcome::Rejected {
                reason: "only a fuel entry has fuel_level_pct".into(),
            });
        }
        if new_level.is_some() && !matches!(fuel_unit.as_deref(), Some("l") | Some("gal")) {
            return Ok(Outcome::Rejected {
                reason: "fuel_level_pct needs a liquid fuel unit".into(),
            });
        }
        if new_quantity.is_some() && !matches!(new_category, "fuel" | "usage") {
            return Ok(Outcome::Rejected {
                reason: "only a fuel entry has quantity_milli".into(),
            });
        }
        if new_quantity.is_some() && fuel_unit.is_none() && counter_unit.is_none() {
            return Ok(Outcome::Rejected {
                reason: "quantity_milli needs an object with a fuel or counter unit".into(),
            });
        }
    }

    Ok(Outcome::Accepted)
}
