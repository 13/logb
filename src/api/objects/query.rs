//! Reading objects back: the owned-row loaders, the derived stats every `ObjectOut` carries,
//! the ancestor chain, and the list and single-object handlers.

use super::*;

/// The one statement both loaders below run, so a column added to `ObjectRow` cannot reach one
/// of them and not the other -- which would show up only as a decode error on whichever path
/// the tests happened not to cover.
const OWNED_OBJECT: &str =
    "SELECT id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
     purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
     energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private \
     FROM objects WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL";

/// The object with `id` if it belongs to `user_id`; otherwise 404. Reads from the pool, for the
/// callers that only want to know the object exists and is theirs before they go on.
///
/// A caller that is about to *write* what it reads here wants `load_owned_object_on` instead:
/// see the comment in `update` for what a read taken before the write lock is worth.
pub async fn load_owned_object(state: &App, user_id: i64, id: i64) -> Result<ObjectRow, AppError> {
    sqlx::query_as::<_, ObjectRow>(OWNED_OBJECT)
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// `load_owned_object` on a connection the caller already holds -- in practice the one
/// `db::begin_write` has just taken the write lock on.
///
/// It has to be the transaction's own connection rather than a second one from the pool: reads
/// and writes are on separate pools now, so a pool connection taken here would not fail or
/// deadlock -- but it would come from a different connection than the one holding the write
/// lock, and the two could disagree about what the row looks like right now. The cost of
/// getting this wrong is holding the writer connection longer than it needs to be held, not a
/// hang.
pub async fn load_owned_object_on(
    conn: &mut sqlx::AnyConnection,
    user_id: i64,
    id: i64,
) -> Result<ObjectRow, AppError> {
    sqlx::query_as::<_, ObjectRow>(OWNED_OBJECT)
        .bind(id)
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(AppError::NotFound)
}

/// One row of derived data per object: the stats block plus the cover's `file_id`.
#[derive(sqlx::FromRow)]
struct DerivedRow {
    object_id: i64,
    total_cost_cents: i64,
    activity_count: i64,
    current_counter: Option<i64>,
    latest_weight_grams: Option<i64>,
    latest_weight_date: Option<String>,
    due_reminder_count: i64,
    last_reading_date: Option<String>,
    last_activity_date: Option<String>,
    /// Filled in after the query, from `usage_by_object`, not read from a column.
    #[sqlx(default)]
    counter_per_day_milli: Option<i64>,
    cover_file_id: Option<i64>,
}

/// Which objects a derived-data query covers: every object a user owns, or a set of ids whose
/// ownership the caller has already checked (the rows a list just read, or the one object a
/// handler loaded through `load_owned_object`).
///
/// This replaced `(user_id: Option<i64>, only: Option<i64>)` pairs spelled into SQL as
/// `($1 IS NULL OR o.user_id = $1) AND ($3 IS NULL OR o.id = $3)`. That form is one statement
/// for every case, and also a statement no planner can use an index for: the choice between
/// "this user" and "this id" is made per row, at run time. Each case now gets SQL of its own.
#[derive(Clone, Copy, Debug)]
pub enum ObjectScope<'a> {
    User(i64),
    Ids(&'a [i64]),
}

/// How many ids one statement names. Far under either backend's bind-parameter limit (SQLite
/// 32766, PostgreSQL 65535) even with the list spelled into a statement more than once, and
/// above the number of objects a household list returns, so the list is one statement.
const IDS_PER_STATEMENT: usize = 1000;

impl<'a> ObjectScope<'a> {
    /// The scope as statements' worth: one for a user, the ids in slices of
    /// `IDS_PER_STATEMENT`, and nothing at all for an empty id list -- which is how a caller
    /// never builds the invalid `IN ()`.
    pub(crate) fn parts(self) -> Vec<ObjectScope<'a>> {
        match self {
            ObjectScope::User(_) => vec![self],
            ObjectScope::Ids(ids) => ids.chunks(IDS_PER_STATEMENT).map(ObjectScope::Ids).collect(),
        }
    }

    /// A predicate on `objects o`, its parameters numbered from `$first`. Whatever it names is
    /// bound, in order, from `binds`.
    pub(crate) fn predicate(self, first: usize) -> String {
        match self {
            ObjectScope::User(_) => format!("o.user_id = ${first}"),
            ObjectScope::Ids(ids) => {
                let list: Vec<String> = (0..ids.len()).map(|i| format!("${}", first + i)).collect();
                format!("o.id IN ({})", list.join(", "))
            }
        }
    }

    pub(crate) fn binds(self) -> Vec<i64> {
        match self {
            ObjectScope::User(user_id) => vec![user_id],
            ObjectScope::Ids(ids) => ids.to_vec(),
        }
    }
}

/// Everything `ObjectOut` needs beyond the `objects` row itself, for every object in `scope`.
/// One copy of this SQL serves the list and the single-object handlers alike.
///
/// The list endpoint used to call `stats` and then a cover lookup once per object, so showing
/// N objects cost 2N + 1 queries. After that it was one statement with nine correlated
/// subqueries per object; now the activities are read in one grouped pass (`agg`), the newest
/// weight by a window over the weighed ones (`weight`), and the due service reminders grouped
/// once (`due`), for only the objects the caller is going to show.
async fn derived(
    state: &App,
    scope: ObjectScope<'_>,
    today: chrono::NaiveDate,
) -> Result<HashMap<i64, DerivedRow>, AppError> {
    // INVARIANT: the `due` CTE is a second, hand-written encoding of
    // `domain::reminder::is_due` -- it exists only so N objects' counts can be computed in one
    // statement instead of loading every reminder and folding `is_due` over them in memory. The
    // two must keep agreeing row for row; `due_reminder_count_agrees_with_each_reminders_due_flag`
    // in tests/it/objects.rs is what catches them drifting apart. Both encodings read the same
    // `today`, the caller's own (`AuthUser::today`), so a user in another zone gets one answer
    // from both.
    //
    // It counts service reminders only. A reading reminder's due date is a calendar-month
    // addition on the latest reading, which SQL cannot spell the same way on both backends, so
    // those are counted in Rust by `due_readings` below with the one copy of the rule.
    //
    // `CAST(SUM(...) AS BIGINT)`, not a bare `SUM`: PostgreSQL widens a sum over a BIGINT
    // column to NUMERIC, which sqlx's `Any` driver cannot decode at all -- every read of an
    // object failed with "Any driver does not support the Postgres type Numeric" before the
    // cast. SQLite reads the cast as INTEGER affinity and is unchanged by it.
    //
    // `due_counter <= agg.current_counter` is NULL, so not due, for an object with no counter
    // reading -- as the correlated `MAX` it replaces was.
    let horizon = super::reminders::reading_horizon(today);
    let today_s = today.to_string();
    let mut rows: HashMap<i64, DerivedRow> = HashMap::new();
    for part in scope.parts() {
        let ids = part.predicate(3);
        let sql = format!(
            "WITH agg AS ( \
               SELECT a.object_id, CAST(SUM(a.cost_cents) AS BIGINT) AS total_cost_cents, \
                 COUNT(*) AS activity_count, MAX(a.counter_value) AS current_counter, \
                 MAX(CASE WHEN a.counter_value IS NOT NULL AND a.date <= $2 THEN a.date END) AS last_reading_date, \
                 MAX(CASE WHEN a.date <= $1 THEN a.date END) AS last_activity_date \
               FROM activities a JOIN objects o ON o.id = a.object_id \
               WHERE a.deleted_at IS NULL AND o.deleted_at IS NULL AND {ids} \
               GROUP BY a.object_id \
             ), weight AS ( \
               SELECT a.object_id, a.weight_grams, a.date, \
                 ROW_NUMBER() OVER (PARTITION BY a.object_id ORDER BY a.date DESC, a.created_at DESC, a.id DESC) AS n \
               FROM activities a JOIN objects o ON o.id = a.object_id \
               WHERE a.deleted_at IS NULL AND a.weight_grams IS NOT NULL AND a.date <= $2 \
                 AND o.deleted_at IS NULL AND {ids} \
             ), due AS ( \
               SELECT r.object_id, COUNT(*) AS n \
               FROM reminders r JOIN objects o ON o.id = r.object_id LEFT JOIN agg ON agg.object_id = r.object_id \
               WHERE r.done_at IS NULL AND r.deleted_at IS NULL AND r.kind = 'service' \
                 AND o.deleted_at IS NULL AND {ids} \
                 AND (r.snoozed_until IS NULL OR r.snoozed_until <= $1) AND ( \
                   (r.due_date IS NOT NULL AND r.due_date <= $1) OR \
                   (r.due_counter IS NOT NULL AND r.due_counter <= agg.current_counter)) \
               GROUP BY r.object_id \
             ) \
             SELECT o.id AS object_id, COALESCE(agg.total_cost_cents, 0) AS total_cost_cents, \
               COALESCE(agg.activity_count, 0) AS activity_count, agg.current_counter, \
               weight.weight_grams AS latest_weight_grams, weight.date AS latest_weight_date, \
               COALESCE(due.n, 0) AS due_reminder_count, agg.last_reading_date, agg.last_activity_date, \
               (SELECT file_id FROM attachments WHERE id = o.cover_attachment_id AND deleted_at IS NULL) AS cover_file_id \
             FROM objects o \
               LEFT JOIN agg ON agg.object_id = o.id \
               LEFT JOIN weight ON weight.object_id = o.id AND weight.n = 1 \
               LEFT JOIN due ON due.object_id = o.id \
             WHERE o.deleted_at IS NULL AND {ids}"
        );
        let mut query = sqlx::query_as::<_, DerivedRow>(sqlx::AssertSqlSafe(sql))
            .bind(today_s.clone())
            .bind(horizon.clone());
        for id in part.binds() {
            query = query.bind(id);
        }
        rows.extend(query.fetch_all(&state.db).await?.into_iter().map(|r| (r.object_id, r)));
    }
    if rows.is_empty() {
        return Ok(rows);
    }
    for (object_id, due) in due_readings(state, scope, today).await? {
        if let Some(row) = rows.get_mut(&object_id) {
            row.due_reminder_count += due;
        }
    }
    // One query for every object in scope, the same rate reminders and the Info tab use.
    for (object_id, usage) in super::insights::usage_by_object(state, scope, today).await? {
        if let Some(row) = rows.get_mut(&object_id) {
            row.counter_per_day_milli = Some(usage.rate_milli);
        }
    }
    Ok(rows)
}

/// How many reading reminders are due per object, for the same scope `derived` answers.
/// Decided by `domain::reminder::reading_status`, the rule each reminder's own `due` uses.
async fn due_readings(
    state: &App,
    scope: ObjectScope<'_>,
    today: chrono::NaiveDate,
) -> Result<HashMap<i64, i64>, AppError> {
    use crate::domain::reminder::{reading_status, CalendarSchedule, Every};
    type ReadingRow = (
        i64,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let horizon = super::reminders::reading_horizon(today);
    let mut rows: Vec<ReadingRow> = Vec::new();
    for part in scope.parts() {
        let sql = format!(
            "SELECT r.object_id, r.due_date, r.every_n, r.every_unit, r.snoozed_until, r.schedule, \
               (SELECT MAX(a.date) FROM activities a WHERE a.object_id = r.object_id AND a.deleted_at IS NULL \
                  AND ((o.type = 'body' AND a.weight_grams IS NOT NULL) OR (o.type <> 'body' AND a.counter_value IS NOT NULL)) AND a.date <= $1) AS last_reading_date \
             FROM reminders r JOIN objects o ON o.id = r.object_id \
             WHERE r.kind = 'reading' AND r.done_at IS NULL AND r.deleted_at IS NULL AND o.deleted_at IS NULL \
               AND {}",
            part.predicate(2)
        );
        let mut query = sqlx::query_as::<_, ReadingRow>(sqlx::AssertSqlSafe(sql)).bind(horizon.clone());
        for id in part.binds() {
            query = query.bind(id);
        }
        rows.extend(query.fetch_all(&state.db).await?);
    }
    let parse = |s: &Option<String>| {
        s.as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    };
    let mut counts = HashMap::new();
    for (object_id, start, every_n, every_unit, snoozed, schedule, last) in rows {
        let every = schedule.as_deref().and_then(CalendarSchedule::parse).map(Every::Calendar)
            .or_else(|| Every::from_parts(every_n, every_unit.as_deref()));
        if reading_status(today, parse(&start), parse(&last), every, parse(&snoozed)).0 {
            *counts.entry(object_id).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

impl DerivedRow {
    fn stats(&self) -> ObjectStats {
        ObjectStats {
            total_cost_cents: self.total_cost_cents,
            activity_count: self.activity_count,
            current_counter: self.current_counter,
            latest_weight_grams: self.latest_weight_grams,
            latest_weight_date: self.latest_weight_date.clone(),
            due_reminder_count: self.due_reminder_count,
            last_reading_date: self.last_reading_date.clone(),
            last_activity_date: self.last_activity_date.clone(),
            counter_per_day_milli: self.counter_per_day_milli,
        }
    }

    fn into_out(self, object: ObjectRow) -> ObjectOut {
        let stats = self.stats();
        ObjectOut {
            object,
            stats,
            cover_file_id: self.cover_file_id,
            ancestors: Vec::new(),
        }
    }
}

/// The stats block for one object, for callers outside this module. Ownership is not checked
/// here: every caller has already loaded the object through `load_owned_object`.
pub async fn stats(state: &App, object_id: i64, today: chrono::NaiveDate) -> Result<ObjectStats, AppError> {
    derived(state, ObjectScope::Ids(&[object_id]), today)
        .await?
        .get(&object_id)
        .map(DerivedRow::stats)
        .ok_or(AppError::NotFound)
}

/// The object's ancestor chain, root first, excluding the object itself -- empty when it has
/// no parent, which costs nothing extra: the common case (an object with no parent) never
/// reaches the recursive query at all.
///
/// The walk carries no `user_id` and no `deleted_at` filter, and that is deliberate rather than
/// an oversight -- do not "fix" it here, and do not copy the pattern to a query reached any
/// other way. Three things together are what make it safe, and all three are about the caller:
/// the only caller is `with_stats`, whose row always came from `load_owned_object` (or
/// `load_owned_object_on`), which filters both; every write to `parent_id` goes through
/// `record::parent_is_valid`, which refuses a parent that is not the same user's and undeleted;
/// and `record::cascade_object` tombstones a whole subtree at once, so no live child can outlive
/// a tombstoned ancestor. A chain reached from an object the caller owns is therefore made
/// entirely of undeleted objects the caller owns, and re-filtering would only cost a join. A
/// caller that cannot make all three claims needs the filters.
///
/// Plain `UNION`, not `UNION ALL`, for the same reason `record::parent_is_valid` uses it: a
/// cycle can never be created through either door, but if one ever got into the data anyway
/// `UNION ALL` would walk it forever and hang the read.
///
/// What makes the `UNION` actually terminate is that the CTE projects nothing but
/// `(id, name, parent_id)` -- no depth, no step counter, nothing that distinguishes a
/// revisited row from its first visit. `UNION` deduplicates whole *rows*, so a column that
/// counted the walk would make every row unique by construction and silently defeat the dedup
/// the cycle defence rests on. That is exactly what an earlier version of this query did. The
/// ordering the caller needs is therefore reconstructed in Rust below instead of asked of SQL,
/// and the `remove` that reconstructs it is a second, independent stop: an ancestor already
/// consumed cannot be walked to twice, so no residual data anomaly can spin the Rust loop
/// either, whatever the database returned.
async fn ancestors(state: &App, object: &ObjectRow) -> Result<Vec<(i64, String)>, AppError> {
    let Some(parent_id) = object.parent_id else {
        return Ok(Vec::new());
    };
    let rows: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
        "WITH RECURSIVE chain(id, name, parent_id) AS ( \
           SELECT id, name, parent_id FROM objects WHERE id = $1 \
           UNION \
           SELECT o.id, o.name, o.parent_id FROM objects o \
             JOIN chain c ON o.id = c.parent_id \
         ) SELECT id, name, parent_id FROM chain WHERE id != $1",
    )
    .bind(object.id)
    .fetch_all(&state.db)
    .await?;

    // The query above is unordered -- a set, not a path -- so the chain is rebuilt by following
    // `parent_id` from the object outwards, which yields nearest ancestor first, then reversed
    // for the root-first order the breadcrumb wants.
    let mut by_id: HashMap<i64, (String, Option<i64>)> = rows
        .into_iter()
        .map(|(id, name, parent_id)| (id, (name, parent_id)))
        .collect();
    let mut chain = Vec::with_capacity(by_id.len());
    let mut next = Some(parent_id);
    while let Some(id) = next {
        let Some((name, parent_id)) = by_id.remove(&id) else {
            break;
        };
        next = parent_id;
        chain.push((id, name));
    }
    chain.reverse();
    Ok(chain)
}

pub(super) async fn with_stats(state: &App, user: &AuthUser, object: ObjectRow) -> Result<ObjectOut, AppError> {
    let id = object.id;
    let chain = ancestors(state, &object).await?;
    let mut out = derived(state, ObjectScope::Ids(&[id]), user.today())
        .await?
        .remove(&id)
        .map(|d| d.into_out(object))
        .ok_or(AppError::NotFound)?;
    out.ancestors = chain
        .into_iter()
        .map(|(id, name)| Ancestor { id, name })
        .collect();
    Ok(out)
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub archived: bool,
    /// Absent lists roots only -- the dashboard's contract. An id lists that object's direct
    /// children.
    #[serde(default)]
    pub parent_id: Option<i64>,
    /// Every object the user owns, regardless of nesting, still subject to `archived`.
    #[serde(default)]
    pub all: bool,
}

pub(super) async fn list(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ObjectOut>>, AppError> {
    // `COLLATE NOCASE` is SQLite's spelling; PostgreSQL sorts by `lower(name)`. Without it a
    // list reads as "Banana, apple, cherry", which looks like a bug to the person who typed
    // the names. See `dialect::Backend::name_order`.
    let order = state.backend.name_order("name");
    let archived = if q.archived { "IS NOT NULL" } else { "IS NULL" };
    // Spelled per case rather than as `($3 OR ($2 IS NULL AND ...) OR ...)`, which decides the
    // case per row and leaves the planner no index to use.
    let nesting = match (q.all, q.parent_id) {
        (true, _) => "",
        (false, None) => " AND parent_id IS NULL",
        (false, Some(_)) => " AND parent_id = $2",
    };
    let mut query = sqlx::query_as::<_, ObjectRow>(sqlx::AssertSqlSafe(format!(
        "SELECT id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
         energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private \
         FROM objects WHERE user_id = $1 AND deleted_at IS NULL AND archived_at {archived}{nesting} \
         ORDER BY {order}")))
        .bind(user.id);
    if let (false, Some(parent_id)) = (q.all, q.parent_id) {
        query = query.bind(parent_id);
    }
    let rows = query.fetch_all(&state.db).await?;
    // Empty archived/child lists need no aggregates across the entire account.
    if rows.is_empty() {
        return Ok(Json(Vec::new()));
    }
    // Only for the rows this list shows: a child or archived list used to aggregate the
    // whole account and throw most of it away.
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut derived = derived(&state, ObjectScope::Ids(&ids), user.today()).await?;
    let out = rows
        .into_iter()
        .filter_map(|row| derived.remove(&row.id).map(|d| d.into_out(row)))
        .collect();
    Ok(Json(out))
}

pub(super) async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<Json<ObjectOut>, AppError> {
    let row = load_owned_object(&state, user.id, id).await?;
    Ok(Json(with_stats(&state, &user, row).await?))
}
