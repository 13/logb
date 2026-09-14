//! The sync protocol's vocabulary: what an op can name, and what it may change.

pub mod apply;
pub mod epoch;
pub mod feed;
pub mod record;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Entity { Object, Activity, Reminder, Attachment, File }

impl Entity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Entity::Object => "object",
            Entity::Activity => "activity",
            Entity::Reminder => "reminder",
            Entity::Attachment => "attachment",
            Entity::File => "file",
        }
    }

    /// The table the entity lives in. Kept here rather than interpolated at each call site so
    /// that the only strings ever spliced into SQL come from this closed set.
    pub fn table(&self) -> &'static str {
        match self {
            Entity::Object => "objects",
            Entity::Activity => "activities",
            Entity::Reminder => "reminders",
            Entity::Attachment => "attachments",
            Entity::File => "files",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind { Create, Set, Delete }

impl OpKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            OpKind::Create => "create",
            OpKind::Set => "set",
            OpKind::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Op {
    pub client_op_id: String,
    pub entity: Entity,
    pub entity_uuid: String,
    pub op: OpKind,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    pub edited_at: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum Outcome {
    Accepted,
    Superseded,
    Rejected { reason: String },
}

/// The shape of the column behind a syncable field.
///
/// Only two shapes are reachable over sync: an INTEGER column and a TEXT one. The distinction
/// matters because SQLite does not enforce it -- see `apply::binding`, which is the only place
/// that acts on this, for what a mistyped write costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Integer,
    Text,
}

/// The column type behind `field` on `entity`, or `None` if the field is not settable at all.
///
/// This is the protocol's security boundary, and the reason it is a whitelist rather than a
/// blacklist: `id`, `user_id` and the parent foreign keys are all ordinary columns, so a
/// blacklist that forgot one would let a `set` op move somebody else's row into the caller's
/// account. Anything absent here is simply not settable over sync.
///
/// The type travels with the name so that there is exactly ONE list. A second list -- names
/// here, types elsewhere -- would let a field be added to one and not the other, and the half
/// that drifts is the half that stops being checked.
///
/// The types mirror `migrations/`: everything not named INTEGER there is a TEXT column.
pub fn syncable_field_type(entity: Entity, field: &str) -> Option<FieldType> {
    whitelist(entity).iter().find(|(name, _)| *name == field).map(|(_, ty)| *ty)
}

/// The whitelist itself. Private, so no caller can ask a question about a field other than
/// "may it be written, and as what"; the tests below walk it to pin it against the schema.
fn whitelist(entity: Entity) -> &'static [(&'static str, FieldType)] {
    use FieldType::{Integer, Text};
    match entity {
        Entity::Object => &[
            ("name", Text), ("type", Text), ("counter_unit", Text), ("fuel_unit", Text),
            ("description", Text), ("purchase_date", Text),
            ("purchase_price_cents", Integer), ("archived_at", Text),
            ("cover_attachment_id", Integer), ("parent_id", Integer),
            // JSON text; `apply::canonical_tags` normalises it before it is logged or stored.
            ("tags", Text),
        ],
        Entity::Activity => &[
            ("date", Text), ("category", Text), ("title", Text), ("notes", Text),
            ("counter_value", Integer), ("cost_cents", Integer), ("quantity_milli", Integer),
            ("tags", Text),
        ],
        Entity::Reminder => &[
            ("title", Text), ("notes", Text), ("due_date", Text), ("due_counter", Integer),
            ("repeat_months", Integer), ("repeat_counter", Integer), ("done_at", Text),
            ("done_activity_id", Integer), ("snoozed_until", Text),
            // `kind` is absent on purpose: a reminder never changes kind (see
            // `api::reminders::update`), so it is fixed at create like `object_id`.
            ("every_n", Integer), ("every_unit", Text),
        ],
        Entity::Attachment => &[("kind", Text), ("caption", Text)],
        // Content-addressed and written once. A file changes by being replaced, never edited.
        Entity::File => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_round_trip() {
        assert_eq!(Entity::Activity.as_str(), "activity");
        assert_eq!(Entity::Activity.table(), "activities");
    }

    #[test]
    fn the_whitelist_admits_real_columns() {
        assert!(syncable_field_type(Entity::Object, "name").is_some());
        assert!(syncable_field_type(Entity::Activity, "quantity_milli").is_some());
        assert!(syncable_field_type(Entity::Reminder, "snoozed_until").is_some());
        assert!(syncable_field_type(Entity::Attachment, "caption").is_some());
        assert_eq!(syncable_field_type(Entity::Object, "tags"), Some(FieldType::Text));
        assert_eq!(syncable_field_type(Entity::Activity, "tags"), Some(FieldType::Text));
    }

    #[test]
    fn the_whitelist_refuses_identity_and_ownership() {
        for field in ["id", "user_id", "object_id", "client_uuid", "created_at", "deleted_at"] {
            assert!(
                syncable_field_type(Entity::Object, field).is_none(),
                "{field} must not be settable"
            );
            assert!(
                syncable_field_type(Entity::Activity, field).is_none(),
                "{field} must not be settable"
            );
        }
    }

    /// Every INTEGER column reachable through the whitelist, read off `migrations/`. The list
    /// is spelled out here rather than derived so that the test disagrees with the whitelist
    /// when either one changes alone.
    const INTEGER_COLUMNS: &[(Entity, &str)] = &[
        (Entity::Object, "purchase_price_cents"),
        (Entity::Object, "cover_attachment_id"),
        (Entity::Object, "parent_id"),
        (Entity::Activity, "counter_value"),
        (Entity::Activity, "cost_cents"),
        (Entity::Activity, "quantity_milli"),
        (Entity::Reminder, "due_counter"),
        (Entity::Reminder, "repeat_months"),
        (Entity::Reminder, "repeat_counter"),
        (Entity::Reminder, "done_activity_id"),
        (Entity::Reminder, "every_n"),
    ];

    #[test]
    fn every_whitelisted_field_carries_its_schema_type() {
        let entities = [
            Entity::Object, Entity::Activity, Entity::Reminder, Entity::Attachment, Entity::File,
        ];
        for entity in entities {
            for (field, ty) in whitelist(entity) {
                let integer_in_schema = INTEGER_COLUMNS.contains(&(entity, field));
                let expected = if integer_in_schema { FieldType::Integer } else { FieldType::Text };
                assert_eq!(
                    *ty,
                    expected,
                    "{}.{field} is {expected:?} in migrations/",
                    entity.as_str()
                );
            }
        }
        // The other direction: a field cannot be dropped from the whitelist while still being
        // named above, which would leave the pairing untested rather than failing.
        for (entity, field) in INTEGER_COLUMNS {
            assert_eq!(
                syncable_field_type(*entity, field),
                Some(FieldType::Integer),
                "{}.{field} must still be a syncable integer field",
                entity.as_str()
            );
        }
    }

    #[test]
    fn files_are_immutable() {
        for field in ["sha256", "mime", "size", "original_name"] {
            assert!(
                syncable_field_type(Entity::File, field).is_none(),
                "files are create/delete only"
            );
        }
    }
}
