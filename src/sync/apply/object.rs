//! The cross-field rules a `set` on an object has to pass against its stored row and history.

use crate::error::AppError;
use crate::sync::{Entity, Op, Outcome};
use super::value::Binding;

/// Answers `Outcome::Accepted` when the object's row stays valid with `field` set to `bound`,
/// and the rejection otherwise. Any other entity passes untouched.
pub(super) async fn check_object(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    op: &Op,
    field: &str,
    bound: &Binding,
) -> Result<Outcome, AppError> {
    // The energy price cross-field rule: a price may exist only alongside a fuel unit,
    // checked against the row as it stands right now -- mirrors
    // `ObjectInput::validate`'s combined check on the REST door, for the same reason the
    // `counter_unit`/trip guard below reads the stored row instead of the rest of the
    // PATCH body: a synced `set` touches one field at a time, so the other half of the
    // pair has to come from storage.
    if op.entity == Entity::Object && matches!(field, "energy_price_milli" | "fuel_unit") {
        let (stored_fuel_unit, stored_price): (Option<String>, Option<i64>) =
            sqlx::query_as(
                "SELECT fuel_unit, energy_price_milli FROM objects WHERE client_uuid = $1",
            )
            .bind(&op.entity_uuid)
            .fetch_one(&mut *tx)
            .await?;
        let needs_fuel_unit = if field == "energy_price_milli" {
            !matches!(&bound, Binding::Null) && stored_fuel_unit.is_none()
        } else {
            matches!(&bound, Binding::Null) && stored_price.is_some()
        };
        if needs_fuel_unit {
            return Ok(Outcome::Rejected {
                reason: crate::api::objects::ENERGY_PRICE_NEEDS_FUEL_UNIT.into(),
            });
        }
    }
    if op.entity == Entity::Object && matches!(field, "fuel_capacity_milli" | "fuel_unit") {
        let (stored_fuel_unit, stored_capacity): (Option<String>, Option<i64>) =
            sqlx::query_as(
                "SELECT fuel_unit, fuel_capacity_milli FROM objects WHERE client_uuid = $1",
            )
            .bind(&op.entity_uuid)
            .fetch_one(&mut *tx)
            .await?;
        let unit = if field == "fuel_unit" {
            match &bound {
                Binding::Text(v) => Some(v.as_str()),
                Binding::Null => None,
                _ => stored_fuel_unit.as_deref(),
            }
        } else {
            stored_fuel_unit.as_deref()
        };
        let capacity = if field == "fuel_capacity_milli" {
            !matches!(&bound, Binding::Null)
        } else {
            stored_capacity.is_some()
        };
        if capacity && !matches!(unit, Some("l") | Some("gal")) {
            return Ok(Outcome::Rejected {
                reason: "fuel_capacity_milli needs a liquid fuel unit".into(),
            });
        }
    }
    if op.entity == Entity::Object && field == "fuel_unit" {
        let current: Option<String> =
            sqlx::query_scalar("SELECT fuel_unit FROM objects WHERE client_uuid = $1")
                .bind(&op.entity_uuid)
                .fetch_one(&mut *tx)
                .await?;
        let next = match &bound {
            Binding::Text(v) => Some(v.as_str()),
            Binding::Null => None,
            _ => current.as_deref(),
        };
        if next != current.as_deref() {
            let has_history: Option<(i64,)> = sqlx::query_as(
                "SELECT a.id FROM activities a JOIN objects o ON o.id = a.object_id WHERE o.client_uuid = $1 AND a.deleted_at IS NULL AND (a.quantity_milli IS NOT NULL OR a.fuel_level_pct IS NOT NULL) LIMIT 1")
                .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?;
            if has_history.is_some() {
                return Ok(Outcome::Rejected {
                    reason: crate::api::objects::FUEL_UNIT_HISTORY_REJECTION.into(),
                });
            }
        }
    }

    // A type key needs the database: a built-in key, or one of the caller's own live types
    // -- including one a create earlier in this same push inserted, on this transaction.
    if op.entity == Entity::Object && field == "type" {
        if let Binding::Text(key) = &bound {
            if !crate::object_type::is_valid_for_user(&mut *tx, user_id, key).await? {
                return Ok(Outcome::Rejected {
                    reason: crate::api::objects::TYPE_REJECTION.into(),
                });
            }
        }
    }

    // A trip needs its object to keep counting km or mi -- see the doc comment on
    // `api::objects::COUNTER_UNIT_TRIP_REJECTION`, which both doors answer with. `km`
    // and `mi` are each other's only legal replacement, so this only has to ask about
    // the ones that leave that pair: `h`, and clearing the counter entirely.
    if op.entity == Entity::Object
        && field == "counter_unit"
        && !matches!(&bound, Binding::Text(u) if matches!(u.as_str(), "km" | "mi"))
    {
        let has_trip: Option<(i64,)> = sqlx::query_as(
            "SELECT a.id FROM activities a JOIN objects o ON o.id = a.object_id \
             WHERE o.client_uuid = $1 AND a.category = 'trip' AND a.deleted_at IS NULL LIMIT 1",
        )
        .bind(&op.entity_uuid)
        .fetch_optional(&mut *tx)
        .await?;
        if has_trip.is_some() {
            return Ok(Outcome::Rejected {
                reason: crate::api::objects::COUNTER_UNIT_TRIP_REJECTION.into(),
            });
        }
    }

    Ok(Outcome::Accepted)
}
