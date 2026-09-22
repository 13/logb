//! The custom-type row a sync op creates or renames on its way to a column.

use crate::domain::custom_type::{self, TypeInput};
use crate::error::AppError;
use crate::sync::record;
use crate::sync::{Entity, Op, Outcome};
use super::value::{Binding, CATEGORIES_SHAPE};

/// The four fields a pushed object type `create` carries in `value`.
#[derive(serde::Deserialize)]
pub(super) struct TypeValue {
    name: String,
    icon: String,
    categories: Vec<String>,
    #[serde(default)]
    counter_unit: Option<String>,
}

/// A pushed `create` of an object type. Unlike every other entity's create, which only announces
/// a row already made over REST, this inserts the row. Types are what a device makes offline
/// together with the objects that use them, and the objects refer to the type by this uuid. The
/// same rules as `POST /types` apply: `custom_type::normalize` and a unique name.
///
/// Applied in push order inside the push's one transaction, so a later op in the same batch
/// (`set object.type = custom:<uuid>`) already sees the row through `is_valid_for_user`.
pub(super) async fn create_type(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    op: &Op,
) -> Result<Outcome, AppError> {
    fn rejected(reason: impl Into<String>) -> Result<Outcome, AppError> {
        Ok(Outcome::Rejected {
            reason: reason.into(),
        })
    }
    // Lower case only: the uuid becomes part of an object's `type`, which REST lower-cases, so
    // a mixed-case uuid would name a type no object could ever use.
    let shape_ok = crate::api::normalize_client_uuid(Some(op.entity_uuid.clone()))
        .is_ok_and(|u| u.as_deref() == Some(op.entity_uuid.as_str()));
    if !shape_ok || op.entity_uuid != op.entity_uuid.to_lowercase() {
        return rejected(
            "an object type's entity_uuid must be 8-64 lower-case characters with no whitespace",
        );
    }
    let existing: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT user_id, deleted_at FROM object_types WHERE client_uuid = $1")
            .bind(&op.entity_uuid)
            .fetch_optional(&mut *tx)
            .await?;
    match existing {
        // A replay of a create that already landed, for example through `POST /types`.
        Some((owner, None)) if owner == user_id => return Ok(Outcome::Accepted),
        // Another account's uuid, or a deleted type's: the uuid is spoken for, as on REST.
        Some(_) => return rejected("entity_uuid is already taken"),
        None => {}
    }
    let Some(value) = op.value.clone() else {
        return rejected(
            "an object type create carries name, icon, categories and counter_unit in value",
        );
    };
    let Ok(value) = serde_json::from_value::<TypeValue>(value) else {
        return rejected(
            "an object type create carries name, icon, categories and counter_unit in value",
        );
    };
    let input = TypeInput {
        name: value.name,
        icon: value.icon,
        categories: value.categories,
        counter_unit: value.counter_unit,
    };
    let input = match custom_type::normalize(input) {
        Ok(input) => input,
        Err(reason) => return rejected(reason),
    };
    if crate::api::types::name_taken(tx, user_id, &input.name, None).await? {
        return rejected(crate::api::types::NAME_TAKEN);
    }
    let now = crate::db::now();
    sqlx::query(
        "INSERT INTO object_types (user_id, client_uuid, name, icon, categories, counter_unit, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(user_id).bind(&op.entity_uuid).bind(&input.name).bind(&input.icon)
        .bind(serde_json::to_string(&input.categories).unwrap_or_else(|_| "[]".into()))
        .bind(&input.counter_unit).bind(&now).bind(&now)
        .execute(&mut *tx).await?;
    // Every field was set by this create, at the op's own clock, for the reason
    // `record::record_create` gives: an unstamped field loses to nothing.
    for (field, _) in crate::sync::whitelist(Entity::ObjectType) {
        record::stamp_field_clock(
            tx,
            Entity::ObjectType,
            &op.entity_uuid,
            field,
            &op.edited_at,
            &op.device_id,
        )
        .await?;
    }
    Ok(Outcome::Accepted)
}

/// Checks a pushed `set` on an object type against the whole type: the stored row with this one
/// field replaced goes through `custom_type::normalize`, as a `PATCH /types` body would, and a
/// new name must not collide with the user's other types. Answers the value to store (trimmed,
/// normalised) or the rejection reason.
pub(super) async fn type_field(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    uuid: &str,
    field: &str,
    bound: Binding,
) -> Result<Result<Binding, String>, AppError> {
    let (id, name, icon, categories, counter_unit): (i64, String, String, String, Option<String>) = sqlx::query_as(
        "SELECT id, name, icon, categories, counter_unit FROM object_types WHERE client_uuid = $1")
        .bind(uuid)
        .fetch_one(&mut *tx)
        .await?;
    let mut input = TypeInput {
        name,
        icon,
        categories: serde_json::from_str(&categories).unwrap_or_default(),
        counter_unit,
    };
    let text = match bound {
        Binding::Text(text) => Some(text),
        Binding::Null => None,
        // `binding` has already refused anything but text or null for these TEXT fields.
        Binding::Integer(_) => return Ok(Err(format!("{field} must be a string"))),
    };
    match (field, text) {
        ("counter_unit", unit) => input.counter_unit = unit,
        (_, None) => return Ok(Err(format!("{field} is required"))),
        ("name", Some(text)) => input.name = text,
        ("icon", Some(text)) => input.icon = text,
        ("categories", Some(text)) => match serde_json::from_str::<Vec<String>>(&text) {
            Ok(parsed) => input.categories = parsed,
            Err(_) => return Ok(Err(CATEGORIES_SHAPE.into())),
        },
        _ => return Ok(Err(format!("{field} is not settable"))),
    }
    let input = match custom_type::normalize(input) {
        Ok(input) => input,
        Err(reason) => return Ok(Err(reason.message)),
    };
    if field == "name" && crate::api::types::name_taken(tx, user_id, &input.name, Some(id)).await? {
        return Ok(Err(crate::api::types::NAME_TAKEN.into()));
    }
    Ok(Ok(match field {
        "name" => Binding::Text(input.name),
        "icon" => Binding::Text(input.icon),
        "categories" => {
            Binding::Text(serde_json::to_string(&input.categories).unwrap_or_else(|_| "[]".into()))
        }
        _ => input.counter_unit.map_or(Binding::Null, Binding::Text),
    }))
}

