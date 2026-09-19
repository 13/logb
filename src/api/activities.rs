use super::attachments::{self, AttachmentOut};
use super::objects::{load_owned_object, validate_date, ObjectRow};
use crate::auth::AuthUser;
use crate::db;
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use crate::sync::apply::{canonical_edited_at, wins};
use crate::sync::{record, Entity};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

// Kept in sync with the CHECK on activities.category (migrations 0009, 0011 and 0015 on
// SQLite, 0001, 0002 and 0006 on PostgreSQL) and with frontend/src/lib/types.ts's CATEGORIES:
// four health categories alongside the original seven; `reading`, an entry that is nothing but
// a counter value; and `trip`, an entry whose end is `counter_value` and whose start is the
// `start_counter` column (see the doc comment on `ActivityRow::start_counter`).
pub const CATEGORIES: [&str; 15] = [
    "maintenance", "repair", "purchase", "inspection", "modification", "fuel", "other",
    "symptom", "treatment", "appointment", "medication", "reading", "trip", "weight", "session",
];

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/activities", get(list).post(create))
        .route("/objects/{id}/recent-titles", get(recent_titles))
        .route("/objects/{id}/last-done", get(last_done))
        .route("/activities/{id}", get(read).patch(update).delete(delete))
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ActivityRow {
    pub id: i64,
    pub object_id: i64,
    pub date: String,
    pub category: String,
    pub title: String,
    pub notes: String,
    pub counter_value: Option<i64>,
    pub cost_cents: Option<i64>,
    pub quantity_milli: Option<i64>,
    pub client_op_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The row's sync identity, so a client can name it in an op without a bootstrap first.
    pub client_uuid: Option<String>,
    /// Stored JSON text, answered as the array it holds -- see `ObjectRow::tags`.
    #[serde(serialize_with = "tags::serialize_json_text")]
    pub tags: String,
    /// A trip's start; its end is the existing `counter_value` above, so the object's current
    /// counter, counter reminders and usage per month all keep reading it unchanged. `None` on
    /// every other category -- see `ActivityInput::validate`. Distance (`counter_value -
    /// start_counter`) is never stored, only computed where it is shown.
    pub start_counter: Option<i64>,
    /// A trip's start and end place, trimmed, blank stored as `None`. Used only by `trip`.
    pub from_place: Option<String>,
    pub to_place: Option<String>,
    /// A trip's duration in minutes, 1..=10080 (one week). Used only by `trip`.
    pub duration_minutes: Option<i64>,
    /// A trip's battery used, as a percentage, 0..=100. Used only by `trip`.
    pub battery_used_pct: Option<i64>,
    /// Whether this charge (or fill) topped the battery/tank up: 1 only on a `fuel` entry, and
    /// 0 on every other row (never `NULL` -- see the migration). A flag on the entry rather than
    /// a table, so it travels through sync, export and offline edits like every other field.
    pub charged_full: i64,
    pub weight_grams: Option<i64>,
}

#[derive(Deserialize)]
pub struct ActivityInput {
    pub date: String,
    pub category: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub counter_value: Option<i64>,
    #[serde(default)]
    pub cost_cents: Option<i64>,
    #[serde(default)]
    pub quantity_milli: Option<i64>,
    /// Client-generated id for this creation attempt. Present only from the offline outbox.
    #[serde(default)]
    pub client_op_id: Option<String>,
    /// When an edit was made, for an edit that reaches the server later than that -- one queued
    /// offline. Each field it changes is then written only if nothing newer has changed that
    /// field in the meantime (see `update`). Absent for an ordinary edit, which is made now.
    #[serde(default)]
    pub edited_at: Option<String>,
    /// Identity minted by the client before the server saw the row. A replay carrying the
    /// same value answers with the row the first attempt made. See `super::normalize_client_uuid`.
    #[serde(default)]
    pub client_uuid: Option<String>,
    /// Absent on create means no tags; absent on PATCH keeps the current ones. `validate`
    /// replaces a present list with its normalised form.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Three-state on PATCH, exactly as `ObjectInput::cover_attachment_id`: absent keeps the
    /// stored value (`update` merges it in before calling `validate`, below), `null` clears it,
    /// a value sets it. Absent on create is simply "no value" -- `validate`'s `.flatten()`
    /// reads an absent outer `None` the same way as an explicit `null`.
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub start_counter: Option<Option<i64>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub from_place: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub to_place: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub duration_minutes: Option<Option<i64>>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub battery_used_pct: Option<Option<i64>>,
    /// Absent on create means "not full" (0); absent on PATCH keeps the current value -- the
    /// same rule `tags` follows, not the three-state `double_option` the trip fields use above,
    /// because a charge can never be cleared to "no value", only ever set to 0 or 1.
    #[serde(default)]
    pub charged_full: Option<i64>,
    #[serde(default, deserialize_with = "super::objects::double_option")]
    pub weight_grams: Option<Option<i64>>,
}

/// A trip's place, trimmed; blank becomes `None`. `Err` if what remains is over 80 characters,
/// counted with `chars().count()` rather than bytes so "Bäckerei Müller" is judged by its own
/// 15 letters, not by how many bytes UTF-8 spends on the umlaut.
///
/// `pub(crate)`: `api::export::import` binds the same trimmed spelling into the row it inserts,
/// rather than the archive's raw text, so an import stores the identical value a REST create of
/// the same body would -- see the call there for why `validate_import` alone, which discards
/// the trimmed value it computes, is not enough.
pub(crate) fn trim_place(place: Option<String>) -> Result<Option<String>, AppError> {
    let Some(p) = place else { return Ok(None) };
    let trimmed = p.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > 80 {
        return Err(AppError::BadRequest("from_place and to_place must be at most 80 characters".into()));
    }
    Ok(Some(trimmed.to_string()))
}

/// The one sentence both doors answer a misplaced `charged_full` with -- `ActivityInput::validate`
/// as a 400 and `sync::apply` as a rejection reason -- whichever direction broke it: setting the
/// flag on a non-`fuel` row, or moving a flagged row's category away from `fuel` while it is
/// still stored.
pub const CHARGE_FULL_ONLY: &str = "only a charge can be marked full";

impl ActivityInput {
    pub(crate) fn validate(&mut self, object: &ObjectRow) -> Result<(), AppError> {
        validate_date(&self.date)?;
        if !CATEGORIES.contains(&self.category.as_str()) {
            return Err(AppError::BadRequest(format!("category must be one of {}", CATEGORIES.join(", "))));
        }
        self.title = self.title.trim().to_string();
        // A trip's title defaults to "Trip" ($t('cat.trip')) in the UI when left blank, and a
        // charge's likewise defaults to "Charged"/"Geladen" or the petrol wording
        // ($t(energyLabelKey(fuel_unit))) -- see `activityTitle` on the frontend. Every other
        // category still requires one, exactly as before.
        if self.title.is_empty() && self.category != "trip" && self.category != "fuel" {
            return Err(AppError::BadRequest("title is required".into()));
        }
        let weight = self.weight_grams.flatten();
        if self.category == "weight" {
            if !weight.is_some_and(|w| (1..=1_000_000_000).contains(&w)) {
                return Err(AppError::BadRequest("weight_grams must be between 1 and 1000000000".into()));
            }
            if self.counter_value.is_some() || self.quantity_milli.is_some() || self.cost_cents.is_some() {
                return Err(AppError::BadRequest("weight entries cannot carry counters, fuel or costs".into()));
            }
        } else if weight.is_some() {
            return Err(AppError::BadRequest("only a weight entry has weight_grams".into()));
        }
        if let Some(c) = self.counter_value {
            if object.counter_unit.is_none() {
                return Err(AppError::BadRequest("this object has no counter".into()));
            }
            if c < 0 { return Err(AppError::BadRequest("counter_value must be >= 0".into())); }
        }
        // A reading with no value records nothing at all.
        if self.category == "reading" && self.counter_value.is_none() {
            return Err(AppError::BadRequest("a reading needs counter_value".into()));
        }
        if matches!(self.cost_cents, Some(c) if c < 0) {
            return Err(AppError::BadRequest("cost_cents must be >= 0".into()));
        }
        if let Some(q) = self.quantity_milli {
            if q < 0 { return Err(AppError::BadRequest("quantity_milli must be >= 0".into())); }
            if object.counter_unit.is_none() {
                return Err(AppError::BadRequest("quantity_milli needs an object with a counter".into()));
            }
        }
        if let Some(t) = &self.tags {
            self.tags = Some(tags::normalize(t).map_err(AppError::BadRequest)?);
        }
        if let Some(cf) = self.charged_full {
            if cf != 0 && cf != 1 {
                return Err(AppError::BadRequest("charged_full must be 0 or 1".into()));
            }
            if cf == 1 && self.category != "fuel" {
                return Err(AppError::BadRequest(CHARGE_FULL_ONLY.into()));
            }
        }

        // The five trip fields, resolved to plain values: `.flatten()` reads an absent outer
        // `None` (create, or a PATCH `update` already merged the stored value into) the same
        // way as an explicit `null`. Read with `.take()` and written back below so every
        // caller -- `create`, `update` and `export::validate_import` alike -- can read the
        // trimmed, checked value back off `self` the same way afterwards, by `.flatten()`ing
        // again.
        let start_counter = self.start_counter.take().flatten();
        let duration_minutes = self.duration_minutes.take().flatten();
        let battery_used_pct = self.battery_used_pct.take().flatten();
        let from_place = trim_place(self.from_place.take().flatten())?;
        let to_place = trim_place(self.to_place.take().flatten())?;

        let has_trip_fields = start_counter.is_some() || from_place.is_some() || to_place.is_some()
            || duration_minutes.is_some() || battery_used_pct.is_some();
        if self.category != "trip" && self.category != "session" {
            if has_trip_fields {
                return Err(AppError::BadRequest(
                    "only a trip has start_counter, places, duration or battery".into(),
                ));
            }
        } else if self.category == "trip" {
            // Allowed only on an object with a distance counter, regardless of the object's
            // type's own category list -- see the module doc on `domain::custom_type` and the
            // spec's "Type categories" note: a `trip` is offered by counter unit, not by type.
            if !matches!(object.counter_unit.as_deref(), Some("km") | Some("mi")) {
                return Err(AppError::BadRequest("a trip needs an object that counts km or mi".into()));
            }
            let (Some(end), Some(start)) = (self.counter_value, start_counter) else {
                return Err(AppError::BadRequest("a trip needs start_counter and counter_value".into()));
            };
            if start < 0 || start > end {
                return Err(AppError::BadRequest("start_counter must be between 0 and counter_value".into()));
            }
        } else if start_counter.is_some() || to_place.is_some() || battery_used_pct.is_some() {
            return Err(AppError::BadRequest("a session only has a place and duration".into()));
        }
        if let Some(b) = battery_used_pct {
            if !(0..=100).contains(&b) {
                return Err(AppError::BadRequest("battery_used_pct must be between 0 and 100".into()));
            }
        }
        if let Some(d) = duration_minutes {
            if !(1..=10080).contains(&d) {
                return Err(AppError::BadRequest("duration_minutes must be between 1 and 10080".into()));
            }
        }

        self.start_counter = Some(start_counter);
        self.duration_minutes = Some(duration_minutes);
        self.battery_used_pct = Some(battery_used_pct);
        self.from_place = Some(from_place);
        self.to_place = Some(to_place);
        Ok(())
    }
}

#[derive(Serialize)]
pub struct ActivityOut {
    #[serde(flatten)]
    pub activity: ActivityRow,
    pub attachments: Vec<AttachmentOut>,
}

pub async fn with_attachments(state: &App, rows: Vec<ActivityRow>) -> Result<Vec<ActivityOut>, AppError> {
    let object_id = match rows.first() { Some(r) => r.object_id, None => return Ok(vec![]) };
    let all = attachments::for_object(state, object_id).await?;
    Ok(rows.into_iter().map(|activity| {
        let attachments = all.iter().filter(|a| a.activity_id == Some(activity.id)).cloned().collect();
        ActivityOut { activity, attachments }
    }).collect())
}

async fn one_out(state: &App, row: ActivityRow) -> Result<ActivityOut, AppError> {
    Ok(with_attachments(state, vec![row]).await?.pop().unwrap())
}

pub async fn load_owned_activity(state: &App, user_id: i64, id: i64) -> Result<ActivityRow, AppError> {
    sqlx::query_as::<_, ActivityRow>(
        "SELECT a.id, a.object_id, a.date, a.category, a.title, a.notes, a.counter_value, a.cost_cents, \
         a.quantity_milli, a.client_op_id, a.created_at, a.updated_at, a.client_uuid, a.tags, \
         a.start_counter, a.from_place, a.to_place, a.duration_minutes, a.battery_used_pct, a.charged_full, a.weight_grams \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE a.id = $1 AND o.user_id = $2 AND a.deleted_at IS NULL AND o.deleted_at IS NULL",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

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
    let tag = q.tag.as_deref()?.split_whitespace().collect::<Vec<_>>().join(" ");
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
        return Ok(rows.iter()
            .filter(|(t, _)| wanted_tag.as_ref().is_none_or(|w| tags::carries(t, w)))
            .filter(|(_, title)| wanted_title.as_ref().is_none_or(|w| &fold_title(title) == w))
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

pub async fn list_for_object(state: &App, object_id: i64, q: &ListQuery) -> Result<Vec<ActivityRow>, AppError> {
    if let Some(d) = &q.from { validate_date(d)?; }
    if let Some(d) = &q.to { validate_date(d)?; }
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
    let (sql_limit, sql_offset) = if filtered { (i64::MAX, 0) } else { (limit, offset) };
    let rows = sqlx::query_as::<_, ActivityRow>(
        "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
         start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams \
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
            .filter(|r| wanted_tag.as_ref().is_none_or(|w| tags::carries(&r.tags, w)))
            .filter(|r| wanted_title.as_ref().is_none_or(|w| &fold_title(&r.title) == w))
            .skip(offset as usize).take(limit as usize)
            .collect()
    })
}

async fn list(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Query(q): Query<ListQuery>) -> Result<Response, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let rows = list_for_object(&state, object_id, &q).await?;
    let total = count_for_object(&state, object_id, &q).await?;
    let out = with_attachments(&state, rows).await?;
    Ok((
        [(axum::http::header::HeaderName::from_static("x-total-count"), total.to_string())],
        Json(out),
    ).into_response())
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

async fn recent_titles(
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

async fn last_done(
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
                by_key.insert(key, LastDone {
                    title: title.trim().to_string(),
                    occurrences: 1,
                    last_date: date,
                    last_counter: counter_value,
                    last_activity_id: id,
                });
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
    let open: HashSet<String> = reminder_titles.into_iter().map(|(t,)| fold_title(&t)).collect();

    let mut out: Vec<LastDone> = by_key.into_iter()
        .filter(|(key, row)| row.occurrences >= 2 || open.contains(key))
        .map(|(_, row)| row)
        .collect();
    // Newest `last_date` first, a tie broken by the higher `last_activity_id`. `HashMap`
    // iteration order is unspecified, but `last_activity_id` is unique across rows, so this is a
    // total order regardless of the order `out` started in.
    out.sort_by(|a, b| b.last_date.cmp(&a.last_date).then(b.last_activity_id.cmp(&a.last_activity_id)));
    out.truncate(LAST_DONE_LIMIT);
    Ok(Json(out))
}

async fn create(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Json(mut body): Json<ActivityInput>) -> Result<Response, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    body.validate(&object)?;
    // A blank (or all-whitespace) client_op_id means no idempotency was requested, not a
    // real, indexable id -- see `super::normalize_op_id` for why that distinction matters.
    body.client_op_id = super::normalize_op_id(body.client_op_id.take());
    // A retry after a lost response must resolve to the row the first attempt made. A
    // tombstoned row is deliberately not that row: the op id stays taken (the unique index
    // spans tombstones too), but the activity it named is gone, and handing a deleted row
    // back as if it were live would leak it. `op_id_conflict` is what that case becomes.
    if let Some(op) = body.client_op_id.as_deref() {
        if let Some(existing) = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
             quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
             start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams \
             FROM activities WHERE client_op_id = $1 AND deleted_at IS NULL",
        )
        .bind(op)
        .fetch_optional(&state.db)
        .await?
        {
            return op_id_row_response(&state, existing, object_id).await;
        }
    }
    let client_uuid = super::normalize_client_uuid(body.client_uuid.take())?;
    if let Some(uuid) = client_uuid.as_deref() {
        // Idempotent on the caller's own live row under this object; a conflict on anyone
        // else's, on another object's, or on a tombstone -- see `objects::create`.
        let existing: Option<(i64, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT a.id, a.object_id, o.user_id, a.deleted_at FROM activities a \
             JOIN objects o ON o.id = a.object_id WHERE a.client_uuid = $1")
            .bind(uuid).fetch_optional(&state.db).await?;
        match existing {
            Some((id, oid, owner, None)) if owner == user.id && oid == object_id => {
                let row = load_owned_activity(&state, user.id, id).await?;
                return Ok((StatusCode::OK, Json(one_out(&state, row).await?)).into_response());
            }
            Some(_) => return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }
    let now = db::now();
    let activity_uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let inserted = sqlx::query_as::<_, ActivityRow>(
        "INSERT INTO activities (object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
         start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20) \
         RETURNING id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
         start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams",
    )
    .bind(object_id).bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(&body.client_op_id).bind(&now).bind(&now)
    .bind(&activity_uuid).bind(tags::to_json(body.tags.as_deref().unwrap_or_default()))
    .bind(body.start_counter.flatten()).bind(body.from_place.clone().flatten()).bind(body.to_place.clone().flatten())
    .bind(body.duration_minutes.flatten()).bind(body.battery_used_pct.flatten())
    // Absent on create means "not full" -- see `ActivityInput::charged_full`'s doc comment.
    .bind(body.charged_full.unwrap_or(0)).bind(body.weight_grams.flatten())
    .fetch_one(&mut *tx).await;
    let row = match inserted {
        Ok(row) => row,
        // Two concurrent requests carrying the same client_op_id -- e.g. several browser tabs
        // sharing one offline outbox, all flushing on reconnect -- can both pass the pre-check
        // above before either has inserted. The loser's INSERT then trips the partial unique
        // index instead of the pre-check catching it; treat that exactly like the pre-check
        // would have, by adopting the winner's row rather than failing the request.
        Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
            tx.rollback().await?;
            // Without an op id the only unique index this insert can trip is `client_uuid`:
            // a replay raced past the pre-check above, and the id is spoken for either way.
            let Some(op) = body.client_op_id.as_deref() else {
                return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into()));
            };
            let winner = sqlx::query_as::<_, ActivityRow>(
                "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
                 quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
                 start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams \
                 FROM activities WHERE client_op_id = $1 AND deleted_at IS NULL",
            )
            .bind(op)
            .fetch_optional(&state.db).await?;
            // `fetch_optional` rather than `fetch_one`: the row holding this op id may be a
            // tombstone, which the filter above hides. That is not a 500 -- the id really is
            // taken, so say so.
            let Some(winner) = winner else { return Err(super::op_id_conflict()) };
            return op_id_row_response(&state, winner, object_id).await;
        }
        Err(e) => return Err(e.into()),
    };
    record::record_create(&mut tx, user.id, Entity::Activity, &activity_uuid, &edited_at).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(one_out(&state, row).await?)).into_response())
}

/// The row a client_op_id lookup found -- whether from the pre-check or after losing an
/// insert race -- may belong to a different object than the one being posted to; that's a
/// 409, not this object's row.
async fn op_id_row_response(state: &App, existing: ActivityRow, object_id: i64) -> Result<Response, AppError> {
    if existing.object_id != object_id {
        return Err(super::op_id_conflict());
    }
    Ok((StatusCode::OK, Json(one_out(state, existing).await?)).into_response())
}

async fn read(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ActivityOut>, AppError> {
    let row = load_owned_activity(&state, user.id, id).await?;
    Ok(Json(one_out(&state, row).await?))
}

/// Whether `field` of this activity was changed after `at` by anything else -- a later edit from
/// this or another browser, or a synced device. The same last-write-wins rule a sync `set` op is
/// held to (`sync::apply::wins`), against the same `field_clock`.
async fn changed_since(tx: &mut sqlx::AnyConnection, uuid: &str, field: &str, at: &str) -> Result<bool, AppError> {
    let stored: Option<(String, String)> = sqlx::query_as(
        "SELECT edited_at, device_id FROM field_clock WHERE entity = $1 AND entity_uuid = $2 AND field = $3")
        .bind(Entity::Activity.as_str()).bind(uuid).bind(field)
        .fetch_optional(&mut *tx).await?;
    Ok(stored.is_some_and(|(stored_at, stored_device)| !wins(at, record::DEVICE_ID, &stored_at, &stored_device)))
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(mut body): Json<ActivityInput>) -> Result<Json<ActivityOut>, AppError> {
    let existing = load_owned_activity(&state, user.id, id).await?;
    let object = load_owned_object(&state, user.id, existing.object_id).await?;
    // Trip fields are three-state on PATCH (see `ActivityInput`'s doc comment on
    // `start_counter`): a key the client omitted must keep its stored value, unlike every other
    // field of this handler, which a PATCH always resends in full. Merged in before `validate`
    // so its `.flatten()` reads the stored value back exactly as if the client had sent it.
    if body.start_counter.is_none() { body.start_counter = Some(existing.start_counter); }
    if body.from_place.is_none() { body.from_place = Some(existing.from_place.clone()); }
    if body.to_place.is_none() { body.to_place = Some(existing.to_place.clone()); }
    if body.duration_minutes.is_none() { body.duration_minutes = Some(existing.duration_minutes); }
    if body.battery_used_pct.is_none() { body.battery_used_pct = Some(existing.battery_used_pct); }
    // `charged_full` keeps its stored value when omitted too (see `ActivityInput`'s doc comment
    // on the field), merged in the same way and for the same reason as the trip fields above.
    if body.weight_grams.is_none() { body.weight_grams = Some(existing.weight_grams); }
    if body.charged_full.is_none() { body.charged_full = Some(existing.charged_full); }
    body.validate(&object)?;

    // An edit made offline and sent now carries the moment it was made. Capped at now, so a
    // device whose clock runs ahead cannot make its edit beat every later one for hours.
    let edited_at = match body.edited_at.as_deref() {
        Some(raw) => {
            let at = canonical_edited_at(raw)
                .ok_or_else(|| AppError::BadRequest("edited_at must be an RFC 3339 timestamp".into()))?;
            Some(at.min(record::edited_at_now()))
        }
        None => None,
    };

    let mut tx = db::begin_write(&state.db, state.backend).await?;
    if let Some(at) = &edited_at {
        // Field by field, not all or nothing: a title fixed on the phone at the garage and a cost
        // typed in on the desktop that evening are both kept, whichever arrives last.
        let uuid = record::uuid_of(&mut tx, Entity::Activity, id).await?;
        if body.date != existing.date && changed_since(&mut tx, &uuid, "date", at).await? { body.date = existing.date.clone(); }
        if body.category != existing.category && changed_since(&mut tx, &uuid, "category", at).await? { body.category = existing.category.clone(); }
        if body.title != existing.title && changed_since(&mut tx, &uuid, "title", at).await? { body.title = existing.title.clone(); }
        if body.notes != existing.notes && changed_since(&mut tx, &uuid, "notes", at).await? { body.notes = existing.notes.clone(); }
        if body.counter_value != existing.counter_value && changed_since(&mut tx, &uuid, "counter_value", at).await? { body.counter_value = existing.counter_value; }
        if body.cost_cents != existing.cost_cents && changed_since(&mut tx, &uuid, "cost_cents", at).await? { body.cost_cents = existing.cost_cents; }
        if body.quantity_milli != existing.quantity_milli && changed_since(&mut tx, &uuid, "quantity_milli", at).await? { body.quantity_milli = existing.quantity_milli; }
        // `None` is "keep the stored tags", so losing to a newer edit is spelled by dropping them.
        if body.tags.as_deref().is_some_and(|t| tags::to_json(t) != existing.tags) && changed_since(&mut tx, &uuid, "tags", at).await? { body.tags = None; }
        if body.start_counter.flatten() != existing.start_counter && changed_since(&mut tx, &uuid, "start_counter", at).await? { body.start_counter = Some(existing.start_counter); }
        if body.from_place.clone().flatten() != existing.from_place && changed_since(&mut tx, &uuid, "from_place", at).await? { body.from_place = Some(existing.from_place.clone()); }
        if body.to_place.clone().flatten() != existing.to_place && changed_since(&mut tx, &uuid, "to_place", at).await? { body.to_place = Some(existing.to_place.clone()); }
        if body.duration_minutes.flatten() != existing.duration_minutes && changed_since(&mut tx, &uuid, "duration_minutes", at).await? { body.duration_minutes = Some(existing.duration_minutes); }
        if body.battery_used_pct.flatten() != existing.battery_used_pct && changed_since(&mut tx, &uuid, "battery_used_pct", at).await? { body.battery_used_pct = Some(existing.battery_used_pct); }
        if body.weight_grams.flatten() != existing.weight_grams && changed_since(&mut tx, &uuid, "weight_grams", at).await? { body.weight_grams = Some(existing.weight_grams); }
        if body.charged_full != Some(existing.charged_full) && changed_since(&mut tx, &uuid, "charged_full", at).await? { body.charged_full = Some(existing.charged_full); }
        // Keeping some fields and not others can combine into something no single edit said --
        // a reading without its value, or a trip missing its start -- so the merge is held to
        // the same rules as either edit.
        body.validate(&object)?;
    }

    // Resolved once, after every merge above has had its say, and reused for both the diff and
    // the bind below -- see the doc comment on `ActivityInput::start_counter` for the three
    // states `.flatten()` collapses.
    let start_counter = body.start_counter.flatten();
    let from_place = body.from_place.clone().flatten();
    let to_place = body.to_place.clone().flatten();
    let duration_minutes = body.duration_minutes.flatten();
    let battery_used_pct = body.battery_used_pct.flatten();
    let weight_grams = body.weight_grams.flatten();
    let charged_full = body.charged_full.unwrap_or(0);

    // Only fields whose value actually differs are logged -- a PATCH that rewrites a field
    // with its existing value produces no `changes` row (see `record::record_update`).
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if body.date != existing.date { changed.push(("date", json!(body.date))); }
    if body.category != existing.category { changed.push(("category", json!(body.category))); }
    if body.title != existing.title { changed.push(("title", json!(body.title))); }
    if body.notes != existing.notes { changed.push(("notes", json!(body.notes))); }
    if body.counter_value != existing.counter_value { changed.push(("counter_value", json!(body.counter_value))); }
    if body.cost_cents != existing.cost_cents { changed.push(("cost_cents", json!(body.cost_cents))); }
    if body.quantity_milli != existing.quantity_milli { changed.push(("quantity_milli", json!(body.quantity_milli))); }
    // Logged as the JSON text the column holds, as `objects::update` does.
    let tags = body.tags.as_deref().map(tags::to_json).unwrap_or_else(|| existing.tags.clone());
    if tags != existing.tags { changed.push(("tags", json!(tags))); }
    if start_counter != existing.start_counter { changed.push(("start_counter", json!(start_counter))); }
    if from_place != existing.from_place { changed.push(("from_place", json!(from_place))); }
    if to_place != existing.to_place { changed.push(("to_place", json!(to_place))); }
    if duration_minutes != existing.duration_minutes { changed.push(("duration_minutes", json!(duration_minutes))); }
    if battery_used_pct != existing.battery_used_pct { changed.push(("battery_used_pct", json!(battery_used_pct))); }
    if weight_grams != existing.weight_grams { changed.push(("weight_grams", json!(weight_grams))); }
    if charged_full != existing.charged_full { changed.push(("charged_full", json!(charged_full))); }

    sqlx::query(
        "UPDATE activities SET date = $1, category = $2, title = $3, notes = $4, counter_value = $5, cost_cents = $6, quantity_milli = $7, updated_at = $8, tags = $9, \
         start_counter = $10, from_place = $11, to_place = $12, duration_minutes = $13, battery_used_pct = $14, charged_full = $15, weight_grams = $16 WHERE id = $17 AND deleted_at IS NULL",
    )
    .bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(db::now()).bind(&tags)
    .bind(start_counter).bind(&from_place).bind(&to_place).bind(duration_minutes).bind(battery_used_pct)
    .bind(charged_full).bind(weight_grams)
    .bind(id)
    .execute(&mut *tx).await?;
    if !changed.is_empty() {
        let uuid = record::uuid_of(&mut tx, Entity::Activity, id).await?;
        // The clock records when the edit was made, so a still-older queued edit arriving after
        // this one loses to it too.
        let clock = edited_at.clone().unwrap_or_else(record::edited_at_now);
        record::record_update(&mut tx, user.id, Entity::Activity, &uuid, &changed, &clock).await?;
    }
    tx.commit().await?;

    let row = load_owned_activity(&state, user.id, id).await?;
    Ok(Json(one_out(&state, row).await?))
}

/// As with objects, the row is tombstoned rather than removed, and the cascades `ON DELETE
/// CASCADE` / `ON DELETE SET NULL` used to provide -- this activity's attachments, and any
/// reminder `done_activity_id` points at it -- are written out by hand, in one transaction so
/// a half-applied delete cannot survive a failure.
///
/// The cover subquery deliberately does *not* skip tombstoned attachments: an object still
/// pointing at one has a stale cover, and clearing it is the whole point of the statement.
async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    load_owned_activity(&state, user.id, id).await?;
    let now = db::now();
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let affected = sqlx::query("UPDATE activities SET deleted_at = $1, updated_at = $2 WHERE id = $3 AND deleted_at IS NULL")
        .bind(&now).bind(&now).bind(id)
        .execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    // `record::cascade_activity` is the single copy of this cascade (the object's cover, a
    // reminder's `done_activity_id`, and the activity's attachments), shared with
    // `sync::apply::apply_op`'s `delete` handling -- see the module docs on `sync::record`.
    let activity_uuid = record::uuid_of(&mut tx, Entity::Activity, id).await?;
    let cascaded = record::cascade_activity(&mut tx, user.id, &activity_uuid, &now, &edited_at).await?;
    record::record_delete(&mut tx, user.id, Entity::Activity, &activity_uuid, &edited_at).await?;
    record::log_cascade(&mut tx, user.id, &edited_at, record::DEVICE_ID, &cascaded).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
