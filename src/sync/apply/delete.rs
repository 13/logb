//! A `delete` op: the row's tombstone and the cascade the schema's `ON DELETE` would have done.

use crate::domain::custom_type::CUSTOM_PREFIX;
use crate::error::AppError;
use crate::sync::record;
use crate::sync::{Entity, Op, Outcome};

/// Tombstones the row an op names, and every row under it; see the `OpKind::Delete` arm of
/// `apply_op` for why the cascade is spelled out here.
pub(super) async fn apply_delete(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    op: &Op,
) -> Result<Outcome, AppError> {
    // A file is content-addressed and shared by every attachment that references it --
    // "a file dies when its last attachment does" is the model `whitelist` already
    // states, and `purge_orphan_files` is what implements it. Tombstoning one directly
    // here would break that silently, in three separate places at once: nothing else
    // in the codebase ever sets `files.deleted_at`, so the purge's `guards` array (see
    // `sync::feed`) has no `files` entry and would never reclaim the tombstone; bootstrap
    // filters `deleted_at IS NULL`, so the file vanishes from every device's snapshot
    // while its still-live attachment keeps pointing at it; and the upload dedup
    // (`api::attachments::create`) has no `deleted_at` filter, so re-uploading the same
    // bytes would re-adopt the dead row and make the replacement photo unrenderable
    // too. Refusing the op here is what keeps all three of those actually true.
    if op.entity == Entity::File {
        return Ok(Outcome::Rejected {
            reason: "files are not deletable over sync".into(),
        });
    }

    // The same refusal as `DELETE /types/{id}`: an object whose type vanished would have no
    // icon and no categories. Counted under the push's write lock, so no object can take
    // the type between this count and the tombstone.
    if op.entity == Entity::ObjectType {
        let in_use: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM objects WHERE user_id = $1 AND type = $2 AND deleted_at IS NULL")
            .bind(user_id).bind(format!("{CUSTOM_PREFIX}{}", op.entity_uuid))
            .fetch_one(&mut *tx).await?;
        if in_use > 0 {
            return Ok(Outcome::Rejected {
                reason: format!("in use by {in_use} object(s)"),
            });
        }
    }

    let now = crate::db::now();
    // Only `objects`, `activities` and `object_types` carry `updated_at`;
    // `reminders`, `attachments` and `files` do not. The REST delete handlers stamp it
    // alongside `deleted_at` wherever the column exists, so this has to too, or a row
    // tombstoned over sync keeps whatever `updated_at` it had before the delete.
    let has_updated_at = matches!(
        op.entity,
        Entity::Object | Entity::Activity | Entity::ObjectType
    );
    let sql = if has_updated_at {
        format!(
            "UPDATE {} SET deleted_at = $1, updated_at = $2 \
             WHERE client_uuid = $3 AND deleted_at IS NULL",
            op.entity.table()
        )
    } else {
        format!(
            "UPDATE {} SET deleted_at = $1 WHERE client_uuid = $2 AND deleted_at IS NULL",
            op.entity.table()
        )
    };
    let query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(&now);
    let query = if has_updated_at {
        query.bind(&now)
    } else {
        query
    };
    query.bind(&op.entity_uuid).execute(&mut *tx).await?;

    let cascaded = match op.entity {
        Entity::Object => record::cascade_object(&mut *tx, &op.entity_uuid, &now).await?,
        Entity::Activity => {
            record::cascade_activity(
                &mut *tx,
                user_id,
                &op.entity_uuid,
                &now,
                &op.edited_at,
            )
            .await?
        }
        // An attachment has no children to tombstone, but it is not a leaf reference-wise:
        // it can be an object's cover, and `cover_attachment_id` is a plain INTEGER with
        // no FK to enforce that by itself -- see `record::clear_cover_of`.
        Entity::Attachment => {
            record::clear_cover_of(&mut *tx, user_id, &op.entity_uuid, &op.edited_at)
                .await?;
            Vec::new()
        }
        // A reminder has no children of its own, and nothing else keeps a stray
        // reference to it that a delete would need to clean up.
        Entity::Reminder => Vec::new(),
        // Nothing points at a type by id; objects that use it were refused above.
        Entity::ObjectType => Vec::new(),
        Entity::File => {
            unreachable!("a file delete is refused above, before reaching this match")
        }
    };
    record::log_cascade(&mut *tx, user_id, &op.edited_at, &op.device_id, &cascaded).await?;
    Ok(Outcome::Accepted)
}
