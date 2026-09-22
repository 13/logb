//! Reading activities back: the list, its filters, and the two summaries built on them.

use crate::api::objects::{load_owned_object, validate_date};
use crate::auth::AuthUser;
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::*;

/// A page of a long timeline. Kept generous because the common object has tens of
/// activities, not thousands; the cap only exists so a decade-old car cannot make the
/// dashboard ship megabytes to a phone in one response.
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

#[derive(Deserialize)]
pub struct ListQuery {
    pub category: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    /// Only entries carrying this tag, compared ignoring case and accents.
    #[serde(default)]
    pub tag: Option<String>,
    /// Only entries whose title matches this one exactly, ignoring case and surrounding space.
    #[serde(default)]
    pub title: Option<String>,
}

/// The `tag` filter as the key `domain::tags::fold` compares by, or `None` when there is no
/// filter. A blank value is no filter, not a filter nothing matches.
fn wanted_tag(q: &ListQuery) -> Option<String> {
    let tag = q
        .tag
        .as_deref()?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!tag.is_empty()).then(|| tags::fold(&tag))
}

/// The key two titles are compared under: trimmed, then lowercased in Rust rather than SQL.
/// SQLite's `LOWER()` folds ASCII only (this build has no ICU extension), so `LOWER(TRIM(title))`
/// would leave "BREMSBELÄGE" and "Bremsbeläge" as different groups there while PostgreSQL's
/// (Unicode-aware) `LOWER()` would merge them -- the same query would then answer differently
/// depending only on which database happens to be configured. `str::to_lowercase` performs the
/// same Unicode-aware lowercasing in the application instead, so it runs identically on both
/// backends. It is still only lowercasing, not full Unicode case *folding*: "STRASSE" and
/// "Straße" do not match (folding would map both to "strasse"; `to_lowercase` leaves the ß),
/// which is fine here -- the spec asks for case-insensitivity, not a ß/ss equivalence, and both
/// `last_done`'s grouping and the `title` filter below call this one helper either way, so a
/// title tapped in one always matches what the other shows for it.
///
/// `pub(crate)`: `api::trips::distinct_places` folds a trip's from/to places by this same key,
/// so "Home" and "home" collapse into one suggestion the same way two title spellings collapse
/// into one `last_done` entry, rather than a second, possibly-diverging fold living there.
pub(crate) fn fold_title(title: &str) -> String {
    title.trim().to_lowercase()
}

/// The `title` filter as the key `fold_title` compares by, or `None` when there is no filter. A
/// blank (or all-whitespace) value is no filter, not a filter nothing matches.
fn wanted_title(q: &ListQuery) -> Option<String> {
    let title = q.title.as_deref()?.trim();
    (!title.is_empty()).then(|| fold_title(title))
}

/// How many activities match the filters, ignoring the page window -- the client needs it to
/// know whether a "load more" button belongs on screen.
pub async fn count_for_object(state: &App, object_id: i64, q: &ListQuery) -> Result<i64, AppError> {
    let wanted_tag = wanted_tag(q);
    let wanted_title = wanted_title(q);
    if wanted_tag.is_some() || wanted_title.is_some() {
        // Counted in Rust with the same match the page uses, so the header and the page can
        // never disagree -- see `list_for_object` for why SQL cannot do this match.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT tags, title FROM activities WHERE object_id = $1 AND deleted_at IS NULL \
             AND ($2 IS NULL OR category = $2) AND ($3 IS NULL OR date >= $3) AND ($4 IS NULL OR date <= $4)",
        )
        .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to)
        .fetch_all(&state.db).await?;
        return Ok(rows
            .iter()
            .filter(|(t, _)| wanted_tag.as_ref().is_none_or(|w| tags::carries(t, w)))
            .filter(|(_, title)| {
                wanted_title
                    .as_ref()
                    .is_none_or(|w| &fold_title(title) == w)
            })
            .count() as i64);
    }
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM activities WHERE object_id = $1 AND deleted_at IS NULL \
         AND ($2 IS NULL OR category = $2) AND ($3 IS NULL OR date >= $3) AND ($4 IS NULL OR date <= $4)",
    )
    .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to)
    .fetch_one(&state.db).await?;
    Ok(n)
}

pub async fn list_for_object(
    state: &App,
    object_id: i64,
    q: &ListQuery,
) -> Result<Vec<ActivityRow>, AppError> {
    if let Some(d) = &q.from {
        validate_date(d)?;
    }
    if let Some(d) = &q.to {
        validate_date(d)?;
    }
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = q.offset.unwrap_or(0).max(0);
    // A tag matches ignoring case and accents, and a title matches ignoring case and surrounding
    // space -- neither of which either backend's LIKE/LOWER does reliably (SQLite's LOWER folds
    // ASCII only, and neither strips accents; see `fold_title`). So with either filter SQL
    // returns every row the other filters allow and the match and the page window are applied
    // here. One object's timeline is hundreds of rows at most, so that costs nothing noticeable.
    let wanted_tag = wanted_tag(q);
    let wanted_title = wanted_title(q);
    let filtered = wanted_tag.is_some() || wanted_title.is_some();
    let (sql_limit, sql_offset) = if filtered {
        (i64::MAX, 0)
    } else {
        (limit, offset)
    };
    let rows = sqlx::query_as::<_, ActivityRow>(
        "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
         start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams, fuel_level_pct, meter_reading_milli, period_start, period_end, estimated, meter_reset \
         FROM activities WHERE object_id = $1 AND deleted_at IS NULL \
         AND ($2 IS NULL OR category = $2) AND ($3 IS NULL OR date >= $3) AND ($4 IS NULL OR date <= $4) \
         ORDER BY date DESC, id DESC LIMIT $5 OFFSET $6",
    )
    .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to).bind(sql_limit).bind(sql_offset)
    .fetch_all(&state.db).await?;
    Ok(if !filtered {
        rows
    } else {
        rows.into_iter()
            .filter(|r| {
                wanted_tag
                    .as_ref()
                    .is_none_or(|w| tags::carries(&r.tags, w))
            })
            .filter(|r| {
                wanted_title
                    .as_ref()
                    .is_none_or(|w| &fold_title(&r.title) == w)
            })
            .skip(offset as usize)
            .take(limit as usize)
            .collect()
    })
}

pub(crate) async fn list(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
    Query(q): Query<ListQuery>,
) -> Result<Response, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let rows = list_for_object(&state, object_id, &q).await?;
    let total = count_for_object(&state, object_id, &q).await?;
    let out = with_attachments(&state, rows).await?;
    Ok((
        [(
            axum::http::header::HeaderName::from_static("x-total-count"),
            total.to_string(),
        )],
        Json(out),
    )
        .into_response())
}

/// One row per distinct (title, category) an object has seen, newest first, with the
/// values of the most recent occurrence.
///
/// A dedicated endpoint rather than reusing `list`: that one joins every attachment of
/// the object, which is payload a phone does not need in order to fill a datalist.
#[derive(Serialize, sqlx::FromRow)]
pub struct TitleSuggestion {
    pub title: String,
    pub category: String,
    pub last_date: String,
    pub last_cost_cents: Option<i64>,
    pub last_counter: Option<i64>,
    /// The newest occurrence's trip places -- always `None` for any other category, the same
    /// way `from_place`/`to_place` are `None` on the stored row itself. Lets the frontend's
    /// "Repeat" chip prefill From/To for a trip exactly as it already does title/category/cost.
    pub last_from_place: Option<String>,
    pub last_to_place: Option<String>,
}

const SUGGESTION_LIMIT: i64 = 20;

pub(crate) async fn recent_titles(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
) -> Result<Json<Vec<TitleSuggestion>>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    // The correlated subqueries pick the newest occurrence explicitly. SQLite would also
    // hand back a bare column from the MAX() row, but that behaviour is a quirk to rely on,
    // not a contract.
    //
    // `a.object_id` is in the GROUP BY only so the subqueries may name it: PostgreSQL refuses
    // an ungrouped outer column inside a subquery, while SQLite allows it. It groups nothing
    // differently -- the WHERE clause has already pinned `object_id` to a single value -- so
    // the rows are the same on both backends.
    let rows = sqlx::query_as::<_, TitleSuggestion>(
        "SELECT a.title, a.category, MAX(a.date) AS last_date, \
           (SELECT x.cost_cents FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category AND x.deleted_at IS NULL \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_cost_cents, \
           (SELECT x.counter_value FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category AND x.deleted_at IS NULL \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_counter, \
           (SELECT x.from_place FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category AND x.deleted_at IS NULL \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_from_place, \
           (SELECT x.to_place FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category AND x.deleted_at IS NULL \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_to_place \
         FROM activities a WHERE a.object_id = $1 AND a.deleted_at IS NULL \
         GROUP BY a.object_id, a.title, a.category ORDER BY last_date DESC LIMIT $2",
    )
    .bind(object_id)
    .bind(SUGGESTION_LIMIT)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// One row per title an object has seen more than once (or once, if it also has an open
/// reminder of the same title), newest occurrence first -- see `last_done` and section C of
/// `docs/superpowers/specs/2026-09-15-dates-tags-last-done-design.md`.
#[derive(Serialize, Clone, Debug)]
pub struct LastDone {
    /// The spelling of the newest occurrence, trimmed.
    pub title: String,
    pub occurrences: i64,
    pub last_date: String,
    pub last_counter: Option<i64>,
    pub last_activity_id: i64,
}

const LAST_DONE_LIMIT: usize = 50;

pub(crate) async fn last_done(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
) -> Result<Json<Vec<LastDone>>, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    // Grouped in Rust by `fold_title`, not by a SQL GROUP BY on a folded expression -- see the
    // comment on `fold_title` for why SQL's own folding would disagree between backends. Rows
    // arrive newest first, so the first one seen for a given key is already that title's newest
    // occurrence, and any later one for the same key only adds to the count.
    //
    // `trip` is always excluded: a trip is something logged, not something done, and its title
    // is optional besides. `fuel` is excluded only when the object actually has a `fuel_unit` --
    // that object gets an Energy section instead, where a charge's own figures belong, and a
    // charge's row would otherwise show the meaningless "0 km ago" every single time. An object
    // that offers the `fuel` category (car/e_bike/motorcycle, regardless of `fuel_unit`) but has
    // none set gets no Energy section at all, so "distance since the last fill" still belongs
    // here for it, exactly as it did before the Energy section existed.
    let entry_rows: Vec<(i64, String, String, Option<i64>)> = if object.fuel_unit.is_some() {
        sqlx::query_as(
            "SELECT id, title, date, counter_value FROM activities \
             WHERE object_id = $1 AND deleted_at IS NULL AND category NOT IN ('reading', 'trip', 'fuel', 'weight') \
             ORDER BY date DESC, id DESC",
        )
        .bind(object_id)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, title, date, counter_value FROM activities \
             WHERE object_id = $1 AND deleted_at IS NULL AND category NOT IN ('reading', 'trip', 'weight') \
             ORDER BY date DESC, id DESC",
        )
        .bind(object_id)
        .fetch_all(&state.db)
        .await?
    };

    let mut by_key: HashMap<String, LastDone> = HashMap::new();
    for (id, title, date, counter_value) in entry_rows {
        let key = fold_title(&title);
        match by_key.get_mut(&key) {
            Some(existing) => existing.occurrences += 1,
            None => {
                by_key.insert(
                    key,
                    LastDone {
                        title: title.trim().to_string(),
                        occurrences: 1,
                        last_date: date,
                        last_counter: counter_value,
                        last_activity_id: id,
                    },
                );
            }
        }
    }

    // An open reminder names something worth doing again even the first time it was ever
    // logged, so a single occurrence still belongs on this list when one exists for its title. A
    // title with no occurrence at all never entered `by_key` above and so cannot appear here
    // either -- there is no "newest occurrence" a bare reminder could source `last_date` from.
    let reminder_titles: Vec<(String,)> = sqlx::query_as(
        "SELECT title FROM reminders WHERE object_id = $1 AND done_at IS NULL AND deleted_at IS NULL",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;
    let open: HashSet<String> = reminder_titles
        .into_iter()
        .map(|(t,)| fold_title(&t))
        .collect();

    let mut out: Vec<LastDone> = by_key
        .into_iter()
        .filter(|(key, row)| row.occurrences >= 2 || open.contains(key))
        .map(|(_, row)| row)
        .collect();
    // Newest `last_date` first, a tie broken by the higher `last_activity_id`. `HashMap`
    // iteration order is unspecified, but `last_activity_id` is unique across rows, so this is a
    // total order regardless of the order `out` started in.
    out.sort_by(|a, b| {
        b.last_date
            .cmp(&a.last_date)
            .then(b.last_activity_id.cmp(&a.last_activity_id))
    });
    out.truncate(LAST_DONE_LIMIT);
    Ok(Json(out))
}

