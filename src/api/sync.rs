//! The sync endpoints. Thin: every decision lives in `crate::sync`.

use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use crate::sync::feed;
use crate::sync::record;
use crate::sync::{
    apply::{apply_op, canonical_edited_at, canonical_value},
    Op, OpKind, Outcome,
};
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn router() -> Router<App> {
    Router::new()
        .route("/sync/push", post(push))
        .route("/sync/pull", get(pull))
        .route("/sync/bootstrap", get(bootstrap))
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
    // On SQLite this is `BEGIN IMMEDIATE` rather than the default deferred begin: push is the
    // endpoint most likely to have concurrent writers, and a deferred transaction that is
    // going to write can lose its snapshot under WAL and throw the whole batch away with a
    // 500. PostgreSQL spells the same intention as a plain `BEGIN` -- and rejects SQLite's
    // spelling as a syntax error -- and additionally needs the advisory lock that makes it the
    // only writer, so both halves come from `db::begin_write`.
    let mut tx = crate::db::begin_write(&state).await?;
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

        // The schema documents `field TEXT, -- NULL for create and delete`: only a `set` op
        // names a field or carries a value, so anything a `create`/`delete` op happened to
        // have in those spots is junk that must not be stored, or pull would serve it back to
        // other devices as if it meant something. (A `set` with no value at all is a different
        // thing entirely -- that is a field being cleared, and `apply::binding` reads it as
        // NULL -- so the two are kept apart rather than bundled.)
        let (field, value) = if op.op == OpKind::Set {
            // The log must carry what will be stored, not the device's spelling -- see
            // `apply::canonical_value`.
            op.value = canonical_value(op.entity, op.field.as_deref(), op.value.take());
            (op.field.as_deref(), op.value.as_ref().map(|v| v.to_string()))
        } else {
            (None, None)
        };

        // Idempotency: an op id already in the log was applied by an earlier attempt whose
        // response the client never saw. Report it as accepted without applying it twice.
        //
        // The log row goes in BEFORE the op is applied, and the INSERT is what answers the
        // question -- `idx_changes_user_op` is the authority on whether an id has been seen,
        // not a read. This used to be `SELECT seq ...` followed by an INSERT, and a pair of
        // statements can be overtaken between them: a client retrying a push it never got an
        // answer to can have both attempts in flight at once, both read "not seen", both
        // insert, and the loser hits the index and turns the whole batch into a 500 -- for a
        // client whose only recourse is to retry, forever. One statement cannot be overtaken.
        // Whoever inserts the row owns the op; whoever inserts nothing is looking at an
        // attempt that already applied it, and answers `accepted` for it without applying it
        // a second time. What serialises writes today (`BEGIN IMMEDIATE` on SQLite, the
        // advisory lock in `db::begin_write` on PostgreSQL) means the two rarely meet -- but
        // idempotency should not be resting on a lock taken for a different reason.
        //
        // Scoped by user because clients mint their own op ids, so an id is only unique within
        // the account that minted it. Unscoped, one account reusing an id another had already
        // used would be told `accepted` while its write was never applied -- a lost write
        // reported as success. `idx_changes_user_op` makes the log agree: uniqueness is on
        // (user_id, client_op_id), which is exactly what this insert conflicts on.
        let claimed = sqlx::query(
            // `seq` is left to the column's own default here too, matching
            // `record::insert_change` -- see the comment there. It has to be left to the
            // default in *both* places or neither: PostgreSQL's identity sequence does not
            // advance when a value is supplied, so one site assigning its own `seq` while
            // the other took the default would hand out the same number twice.
            "INSERT INTO changes \
             (entity, entity_uuid, op, field, value, edited_at, applied_at, user_id, \
              device_id, client_op_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (user_id, client_op_id) DO NOTHING")
            .bind(op.entity.as_str())
            .bind(&op.entity_uuid)
            .bind(op.op.as_str())
            .bind(field)
            .bind(value)
            .bind(&op.edited_at)
            .bind(db::now())
            .bind(user.id)
            .bind(&op.device_id)
            .bind(&op.client_op_id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        if !claimed {
            results.push(OpResult {
                client_op_id: op.client_op_id.clone(),
                outcome: Outcome::Accepted,
            });
            continue;
        }

        let outcome = apply_op(&mut tx, user.id, op).await?;
        if matches!(outcome, Outcome::Rejected { .. }) {
            // A rejected op is not part of the log -- it changed nothing -- so the claim above
            // has to be given back. Left standing it would answer the client's next attempt
            // `accepted` for a write that never happened, which is the lost write this whole
            // mechanism exists to prevent. The unique index names exactly one row, so this
            // deletes the claim and nothing else. It burns a `seq`, which costs nothing: the
            // column has never promised to be gapless (see `record::insert_change`), only to
            // be increasing.
            sqlx::query("DELETE FROM changes WHERE user_id = $1 AND client_op_id = $2")
                .bind(user.id)
                .bind(&op.client_op_id)
                .execute(&mut *tx)
                .await?;
        } else {
            // The table name comes from `Entity::table`, a closed set, and the uuid stays a
            // bind parameter -- which is the audit `AssertSqlSafe` requires of the caller.
            let sql = format!("SELECT id FROM {} WHERE client_uuid = $1", op.entity.table());
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
    Ok(Json(PushOut { results, server_time: record::edited_at_now(), ids }))
}

/// A page big enough that a normal catch-up is one round trip, small enough that a phone on a
/// bad connection is not asked to hold a huge response in memory.
const DEFAULT_LIMIT: i64 = 500;
const MAX_LIMIT: i64 = 1000;

#[derive(Deserialize)]
pub struct PullParams {
    #[serde(default)]
    pub since: i64,
    pub limit: Option<i64>,
    pub epoch: Option<String>,
}

#[derive(Serialize)]
pub struct PullOut {
    pub changes: Vec<feed::ChangeRow>,
    pub next_seq: i64,
    /// False when the page filled exactly, meaning the client should pull again immediately.
    pub complete: bool,
    pub server_time: String,
    /// Which database these seq numbers belong to. A client stores it beside its cursor.
    pub epoch: String,
}

async fn pull(
    State(state): State<App>,
    user: AuthUser,
    Query(params): Query<PullParams>,
) -> Result<Json<PullOut>, AppError> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let horizon = feed::horizon(&state.db, user.id).await?;
    // `since` of 0 is a first pull and always legal.
    //
    // An empty log (horizon 0) with a non-zero cursor is a failure of its own: a client only
    // ever gets a non-zero cursor from ops that existed, so if this user has none left they
    // were purged. Without this arm that client is handed 200 and an empty page, and silently
    // carries on believing it is current.
    //
    // Below that, staleness is judged against `retention_floor`, not `horizon` -- see that
    // function's comment for why. In short: `horizon` is this user's own oldest surviving row,
    // and `seq` is shared with every other account, so the numbers between this user's own rows
    // are routinely someone else's, or a push rejection's burned claim, and never had anything
    // of this user's to lose either way. `horizon` cannot tell those apart from a genuinely
    // purged row of this user's own; comparing against the floor does not need to.
    let floor = feed::retention_floor(&state.db).await?;
    if params.since > 0 && (horizon == 0 || params.since < floor - 1) {
        return Err(AppError::Gone);
    }
    let epoch = crate::sync::epoch::current(&state.db).await?;
    // A first pull (`since` 0) has no cursor to invalidate, so it needs no epoch. Any other
    // cursor is a claim about a specific database, and a client that cannot back that claim --
    // wrong epoch, or none at all -- must not be allowed to resume against seq numbers that may
    // since have been reissued to different ops.
    if params.since > 0 && params.epoch.as_deref() != Some(epoch.as_str()) {
        return Err(AppError::Gone);
    }
    let changes = feed::pull(&state.db, user.id, params.since, limit).await?;
    let complete = (changes.len() as i64) < limit;
    let next_seq = changes.last().map(|c| c.seq).unwrap_or(params.since);
    Ok(Json(PullOut { changes, next_seq, complete, server_time: record::edited_at_now(), epoch }))
}

async fn bootstrap(
    State(state): State<App>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (seq, mut snapshot) = feed::snapshot(&state.db, user.id).await?;
    let map = snapshot.as_object_mut().expect("snapshot builds a JSON object");
    map.insert("seq".into(), serde_json::json!(seq));
    map.insert("server_time".into(), serde_json::json!(record::edited_at_now()));
    map.insert("epoch".into(), serde_json::json!(crate::sync::epoch::current(&state.db).await?));
    Ok(Json(snapshot))
}
