//! The sync protocol's vocabulary: what an op can name, and what it may change.

pub mod apply;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Entity { Object, Activity, Reminder, Attachment, File }

// `from_str` here returns Option, not the Result that `std::str::FromStr` demands: an unknown
// entity name is an ordinary "not one of ours" answer on a parse of untrusted input, not an
// error worth a type. Naming it anything else would just make every call site read worse.
#[allow(clippy::should_implement_trait)]
impl Entity {
    pub fn from_str(s: &str) -> Option<Entity> {
        match s {
            "object" => Some(Entity::Object),
            "activity" => Some(Entity::Activity),
            "reminder" => Some(Entity::Reminder),
            "attachment" => Some(Entity::Attachment),
            "file" => Some(Entity::File),
            _ => None,
        }
    }

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

// `from_str` here returns Option, not the Result that `std::str::FromStr` demands: an unknown
// op kind name is an ordinary "not one of ours" answer on a parse of untrusted input, not an
// error worth a type. Naming it anything else would just make every call site read worse.
#[allow(clippy::should_implement_trait)]
impl OpKind {
    pub fn from_str(s: &str) -> Option<OpKind> {
        match s {
            "create" => Some(OpKind::Create),
            "set" => Some(OpKind::Set),
            "delete" => Some(OpKind::Delete),
            _ => None,
        }
    }

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

/// Whether an op may write `field` on `entity`.
///
/// This is the protocol's security boundary, and the reason it is a whitelist rather than a
/// blacklist: `id`, `user_id` and the parent foreign keys are all ordinary columns, so a
/// blacklist that forgot one would let a `set` op move somebody else's row into the caller's
/// account. Anything absent here is simply not settable over sync.
pub fn is_syncable_field(entity: Entity, field: &str) -> bool {
    let allowed: &[&str] = match entity {
        Entity::Object => &[
            "name", "category", "counter_unit", "fuel_unit", "description",
            "purchase_date", "purchase_price_cents", "archived_at", "cover_attachment_id",
        ],
        Entity::Activity => &[
            "date", "category", "title", "notes", "counter_value", "cost_cents",
            "quantity_milli",
        ],
        Entity::Reminder => &[
            "title", "notes", "due_date", "due_counter", "repeat_months", "repeat_counter",
            "done_at", "done_activity_id", "snoozed_until",
        ],
        Entity::Attachment => &["kind", "caption"],
        // Content-addressed and written once. A file changes by being replaced, never edited.
        Entity::File => &[],
    };
    allowed.contains(&field)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_round_trip() {
        assert_eq!(Entity::from_str("activity"), Some(Entity::Activity));
        assert_eq!(Entity::Activity.as_str(), "activity");
        assert_eq!(Entity::Activity.table(), "activities");
        assert_eq!(Entity::from_str("users"), None);
    }

    #[test]
    fn the_whitelist_admits_real_columns() {
        assert!(is_syncable_field(Entity::Object, "name"));
        assert!(is_syncable_field(Entity::Activity, "quantity_milli"));
        assert!(is_syncable_field(Entity::Reminder, "snoozed_until"));
        assert!(is_syncable_field(Entity::Attachment, "caption"));
    }

    #[test]
    fn the_whitelist_refuses_identity_and_ownership() {
        for field in ["id", "user_id", "object_id", "client_uuid", "created_at", "deleted_at"] {
            assert!(!is_syncable_field(Entity::Object, field), "{field} must not be settable");
            assert!(!is_syncable_field(Entity::Activity, field), "{field} must not be settable");
        }
    }

    #[test]
    fn files_are_immutable() {
        for field in ["sha256", "mime", "size", "original_name"] {
            assert!(!is_syncable_field(Entity::File, field), "files are create/delete only");
        }
    }
}
