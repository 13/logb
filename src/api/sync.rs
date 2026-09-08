//! The sync endpoints. Thin: every decision lives in `crate::sync`.

use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use crate::sync::{
    apply::{apply_op, canonical_edited_at},
    Op, Outcome,
};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn router() -> Router<App> {
    Router::new().route("/sync/push", post(push))
}

#[derive(Deserialize)]
pub struct PushBody {
    pub ops: Vec<Op>,
}

#[derive(Serialize)]
pub struct OpResult {
    pub client_op_id: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Serialize)]
pub struct PushOut {
    pub results: Vec<OpResult>,
    pub server_time: String,
    /// uuid to integer id, so a client can resolve references it only knows by uuid.
    pub ids: HashMap<String, i64>,
}

/// The whole batch lands in one transaction: a client that retries after a lost response must
/// find either all of its ops recorded or none, never a prefix it cannot identify.
///
/// `results` is positional: `results[i]` describes `ops[i]`. Canonicalising the timestamps in
/// a results-emitting pass of its own would break that -- a batch mixing parseable and
/// unparseable `edited_at` values would emit its rejections first, ahead of the results for
/// ops that preceded them -- so the pass below only computes, and the single pass after it
/// emits exactly one result per op, in request order.
async fn push(
    State(state): State<App>,
    user: AuthUser,
    Json(mut body): Json<PushBody>,
) -> Result<Json<PushOut>, AppError> {
    // `BEGIN IMMEDIATE`, not the default deferred begin. A deferred transaction takes its read
    // snapshot first and only asks for the write lock at its first write, so under WAL two
    // devices pushing at once can find the database changed underneath them and get
    // `SQLITE_BUSY_SNAPSHOT` -- which `busy_timeout` does not retry, because waiting cannot fix
    // a stale snapshot. That surfaces as a 500 and the whole batch is thrown away. Push is the
    // endpoint most likely to have concurrent writers, so it takes the write lock up front,
    // where `busy_timeout` does apply.
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let mut ids = HashMap::new();

    // Canonicalise before anything reads the value: the ordering rule, the `field_clock` row
    // it is compared against, and the copy written into `changes` must all be the same shape,
    // or a later comparison is against a string that never went through here. A timestamp that
    // will not parse yields `None` here and becomes this op's rejection below, at its own index.
    let canonical: Vec<Option<String>> = body
        .ops
        .iter()
        .map(|op| canonical_edited_at(&op.edited_at))
        .collect();

    let mut results = Vec::with_capacity(body.ops.len());
    for (op, canonical) in body.ops.iter_mut().zip(canonical) {
        let Some(canonical) = canonical else {
            results.push(OpResult {
                client_op_id: op.client_op_id.clone(),
                outcome: Outcome::Rejected { reason: "edited_at must be RFC3339".into() },
            });
            continue;
        };
        op.edited_at = canonical;

        // Idempotency: an op id already in the log was applied by an earlier attempt whose
        // response the client never saw. Report it as accepted without applying it twice.
        //
        // Scoped by user because clients mint their own op ids, so an id is only unique within
        // the account that minted it. Unscoped, one account reusing an id another had already
        // used would be told `accepted` while its write was never applied -- a lost write
        // reported as success. `idx_changes_user_op` makes the log agree: uniqueness is on
        // (user_id, client_op_id), which is exactly what this lookup asks about.
        let seen: Option<i64> =
            sqlx::query_scalar("SELECT seq FROM changes WHERE user_id = ? AND client_op_id = ?")
                .bind(user.id)
                .bind(&op.client_op_id)
                .fetch_optional(&mut *tx)
                .await?;
        if seen.is_some() {
            results.push(OpResult {
                client_op_id: op.client_op_id.clone(),
                outcome: Outcome::Accepted,
            });
            continue;
        }

        let outcome = apply_op(&mut tx, user.id, op).await?;
        if !matches!(outcome, Outcome::Rejected { .. }) {
            sqlx::query(
                "INSERT INTO changes \
                 (entity, entity_uuid, op, field, value, edited_at, applied_at, user_id, \
                  device_id, client_op_id) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(op.entity.as_str())
                .bind(&op.entity_uuid)
                .bind(op.op.as_str())
                .bind(op.field.as_deref())
                .bind(op.value.as_ref().map(|v| v.to_string()))
                .bind(&op.edited_at)
                .bind(db::now())
                .bind(user.id)
                .bind(&op.device_id)
                .bind(&op.client_op_id)
                .execute(&mut *tx)
                .await?;

            // The table name comes from `Entity::table`, a closed set, and the uuid stays a
            // bind parameter -- which is the audit `AssertSqlSafe` requires of the caller.
            let sql = format!("SELECT id FROM {} WHERE client_uuid = ?", op.entity.table());
            if let Some(id) = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?
            {
                ids.insert(op.entity_uuid.clone(), id);
            }
        }
        results.push(OpResult { client_op_id: op.client_op_id.clone(), outcome });
    }

    tx.commit().await?;
    Ok(Json(PushOut { results, server_time: db::now(), ids }))
}
