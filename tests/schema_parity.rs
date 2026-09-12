//! The two schemas are written by hand, in different dialects, and nothing but this test makes
//! them agree. A column added to one and forgotten in the other does not fail at compile time:
//! it fails at runtime, on whichever instance happens to be running the other backend.
//!
//! Four dimensions are checked, each against the database's own catalogue rather than the
//! migration files -- so it sees what `ALTER TABLE` and the 0009 rebuild actually produced, not
//! what a single file claims:
//!   - table and column names
//!   - nullability (a column NOT NULL on one side and nullable on the other is a defect that
//!     only shows up as a 500 on whichever backend is looser)
//!   - column types, normalised into a small vocabulary (`int`/`text`/`real`/`blob`) -- SQLite
//!     `INTEGER` vs PostgreSQL `BIGINT` is intentional and must NOT be flagged, but `TEXT` vs
//!     `BIGINT` is real drift
//!   - indexes: name, the columns they cover in order, and uniqueness. This is the dimension
//!     that let `idx_api_tokens_user` go missing from the PostgreSQL schema for a release: every
//!     bearer-token authentication did a sequential scan, and the old table/column-only version
//!     of this test passed throughout.

mod common;

use sqlx::AnyPool;
use std::collections::BTreeSet;

/// Everything this test compares about one live database, reduced to sets of strings so the
/// two backends can be diffed the same way regardless of dialect.
struct Schema {
    columns: BTreeSet<String>,
    nullable: BTreeSet<String>,
    types: BTreeSet<String>,
    indexes: BTreeSet<String>,
}

impl Schema {
    /// Just the table names, taken from the `table.column` set rather than asked for with a
    /// query of their own -- so the exclusions made there (`sqlite_%`, `_sqlx_migrations`) hold
    /// here too, instead of being written out a second time and kept in step by hand.
    fn tables(&self) -> BTreeSet<String> {
        self.columns.iter().filter_map(|c| c.split_once('.')).map(|(t, _)| t.to_string()).collect()
    }
}

async fn rows(pool: &AnyPool, sql: &'static str) -> BTreeSet<String> {
    let found: Vec<String> = sqlx::query_scalar(sql).fetch_all(pool).await.unwrap();
    found.into_iter().collect()
}

async fn schema(url: &str) -> Schema {
    let pool = logb::db::connect(url).await.unwrap();
    let sqlite = url.starts_with("sqlite:");

    // Every `table.column`, as the database itself reports it.
    //
    // `sqlite_%` is SQLite's own bookkeeping (`sqlite_sequence`, from AUTOINCREMENT), and
    // `_sqlx_migrations` is the migrator's -- neither is part of the schema under test, and
    // neither has a PostgreSQL counterpart of the same shape.
    let columns_sql = if sqlite {
        "SELECT m.name || '.' || p.name FROM sqlite_master m \
         JOIN pragma_table_info(m.name) p WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' \
         AND m.name <> '_sqlx_migrations'"
    } else {
        "SELECT table_name || '.' || column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'"
    };

    // Every `table.column` that is NULLABLE, on each side.
    //
    // Primary-key columns are excluded from both sides, not just reconciled to agree: SQLite
    // does not enforce NOT NULL on a PRIMARY KEY column unless the column also carries an
    // explicit `NOT NULL` (true even for a single-column, non-INTEGER key such as
    // `sessions.token` -- `pragma_table_info.notnull` reports 0 for it, and SQLite really will
    // store a NULL there if asked to). PostgreSQL always enforces NOT NULL on a PRIMARY KEY.
    // That is a real difference in what the two engines guarantee, but it is not schema drift
    // -- nothing in either migration set asks for a nullable key -- so comparing it here would
    // only ever produce noise on every `id`/`seq`/`token`/`key` column, on every run.
    let nullable_sql = if sqlite {
        "SELECT m.name || '.' || p.name FROM sqlite_master m \
         JOIN pragma_table_info(m.name) p WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' \
         AND m.name <> '_sqlx_migrations' AND p.\"notnull\" = 0 AND p.pk = 0"
    } else {
        "SELECT table_name || '.' || column_name FROM information_schema.columns c \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations' AND is_nullable = 'YES' \
         AND NOT EXISTS ( \
           SELECT 1 FROM information_schema.table_constraints tc \
           JOIN information_schema.key_column_usage kcu \
             ON kcu.constraint_name = tc.constraint_name AND kcu.table_schema = tc.table_schema \
           WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = 'public' \
             AND tc.table_name = c.table_name AND kcu.column_name = c.column_name)"
    };

    // Every `table.column:type`, with the declared type normalised into `int`/`text`/`real`/
    // `blob` on both sides. This is deliberately coarse: SQLite `INTEGER` and PostgreSQL
    // `BIGINT`/`SMALLINT` both fold to `int`, because that gap is intentional (see the comment
    // at the top of `migrations/postgres/0001_schema.sql`) and must not be flagged. A column
    // that is `text` on one side and `int` on the other still shows up as a real disagreement.
    let types_sql = if sqlite {
        "SELECT m.name || '.' || p.name || ':' || CASE \
           WHEN p.type LIKE '%INT%' THEN 'int' \
           WHEN p.type LIKE '%CHAR%' OR p.type LIKE '%CLOB%' OR p.type LIKE '%TEXT%' THEN 'text' \
           WHEN p.type LIKE '%BLOB%' OR p.type = '' THEN 'blob' \
           WHEN p.type LIKE '%REAL%' OR p.type LIKE '%FLOA%' OR p.type LIKE '%DOUB%' THEN 'real' \
           ELSE 'unknown(' || p.type || ')' END \
         FROM sqlite_master m JOIN pragma_table_info(m.name) p \
         WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' AND m.name <> '_sqlx_migrations'"
    } else {
        "SELECT table_name || '.' || column_name || ':' || CASE lower(data_type) \
           WHEN 'bigint' THEN 'int' WHEN 'smallint' THEN 'int' WHEN 'integer' THEN 'int' \
           WHEN 'text' THEN 'text' WHEN 'character varying' THEN 'text' WHEN 'character' THEN 'text' \
           WHEN 'real' THEN 'real' WHEN 'double precision' THEN 'real' WHEN 'numeric' THEN 'real' \
           WHEN 'bytea' THEN 'blob' ELSE 'unknown(' || data_type || ')' END \
         FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'"
    };

    // Every index as `name|unique|col1,col2,...`, columns in declaration order.
    //
    // Both sides exclude the indexes their engine creates implicitly to back a PRIMARY KEY or
    // UNIQUE constraint -- SQLite names these `sqlite_autoindex_<table>_<n>` and never records a
    // `CREATE INDEX` statement for them (`sqlite_master.sql IS NULL` is exactly that case);
    // PostgreSQL names them `<table>_pkey` / `<table>_<cols>_key` and links them to a row in
    // `pg_constraint`. Neither engine lets the schema author name these, so comparing the names
    // would only ever fail. This is the one dimension where the two schemas are allowed to
    // differ silently -- if a *named* index one side declares is ever missing on the other, it
    // is drift, exactly like the `idx_api_tokens_user` incident this test exists to catch.
    //
    // PostgreSQL additionally excludes expression indexes (`pg_index.indexprs IS NOT NULL`).
    // Today that is exactly one index: `idx_users_username_lower`, which is how the PostgreSQL
    // schema enforces the case-insensitive uniqueness that SQLite spells as `UNIQUE COLLATE
    // NOCASE` directly on the `users.username` column -- a per-column collation PostgreSQL has
    // no equivalent of without the `citext` extension, which this project has not taken on. On
    // the SQLite side that constraint's backing index is already excluded by the rule above (it
    // is exactly the kind of implicit unique-constraint autoindex described there), so there is
    // no comparable "named index with an ordered column list" on that side to check this one
    // against. This is a known, permanent, documented blind spot, not an oversight: a column
    // added to `idx_users_username_lower`'s expression, or a second expression index appearing
    // on only one side, would both go uncaught here.
    let indexes_sql = if sqlite {
        "SELECT m.name || '|' || il.\"unique\" || '|' || ( \
           SELECT group_concat(ii.name, ',') FROM ( \
             SELECT name FROM pragma_index_info(m.name) ORDER BY seqno) ii) \
         FROM sqlite_master m JOIN pragma_index_list(m.tbl_name) il ON il.name = m.name \
         WHERE m.type = 'index' AND m.sql IS NOT NULL"
    } else {
        "SELECT i.relname || '|' || (CASE WHEN ix.indisunique THEN '1' ELSE '0' END) || '|' || \
           string_agg(a.attname, ',' ORDER BY k.ord) \
         FROM pg_index ix \
         JOIN pg_class i ON i.oid = ix.indexrelid \
         JOIN pg_class t ON t.oid = ix.indrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum \
         WHERE n.nspname = 'public' \
           AND NOT EXISTS ( \
             SELECT 1 FROM pg_constraint c \
             WHERE c.conindid = ix.indexrelid AND c.contype IN ('p', 'u')) \
           AND ix.indexprs IS NULL \
         GROUP BY i.relname, ix.indisunique"
    };

    let columns = rows(&pool, columns_sql).await;
    let nullable = rows(&pool, nullable_sql).await;
    let types = rows(&pool, types_sql).await;
    let indexes = rows(&pool, indexes_sql).await;
    pool.close().await;

    assert!(!columns.is_empty(), "{url} reported no columns at all -- the migrations did not run");
    Schema { columns, nullable, types, indexes }
}

/// Compares one dimension and, if it disagrees, formats it the way the rest of this test does:
/// grouped, naming what is only on each side, so a failure says what to fix without re-running
/// anything.
fn diff(dimension: &str, sqlite: &BTreeSet<String>, postgres: &BTreeSet<String>) -> Option<String> {
    let only_sqlite: Vec<_> = sqlite.difference(postgres).collect();
    let only_postgres: Vec<_> = postgres.difference(sqlite).collect();
    (!only_sqlite.is_empty() || !only_postgres.is_empty()).then(|| {
        format!(
            "{dimension} disagree.\n  only in SQLite:     {only_sqlite:?}\
             \n  only in PostgreSQL: {only_postgres:?}"
        )
    })
}

/// Compares one database's tables with the list `--copy-to` walks, naming what is on each side
/// and not the other.
///
/// `_sqlx_migrations` is excluded from the schema side (by `schema()` above, on both backends)
/// and is deliberately absent from `TABLES`: it is the migrator's own bookkeeping, and the
/// destination writes its own copy of it when `--copy-to` migrates it before copying anything.
/// Carrying the source's across would either collide with that or leave the destination
/// claiming migrations it never ran.
fn unlisted_tables(backend: &str, schema: &Schema) -> Option<String> {
    let listed: BTreeSet<String> = logb::copy::TABLES.iter().map(|t| (*t).to_string()).collect();
    let tables = schema.tables();
    let uncopied: Vec<_> = tables.difference(&listed).collect();
    let unknown: Vec<_> = listed.difference(&tables).collect();
    (!uncopied.is_empty() || !unknown.is_empty()).then(|| {
        format!(
            "`logb::copy::TABLES` and the {backend} schema disagree.\
             \n  in the schema, copied nowhere:   {uncopied:?}\
             \n  in the list, not in the schema:  {unknown:?}"
        )
    })
}

/// `--copy-to` walks one hand-written list of tables, and so does the verification that proves
/// the copy arrived -- so a table missing from it is copied nowhere *and* never compared. The
/// copy reports success, the operator deletes the source, and the table is gone.
///
/// Nothing in `src/copy.rs` can catch that: it takes a real database to say what tables the
/// schema has. This is that database. SQLite alone, so it runs on every suite run rather than
/// only where a PostgreSQL server is configured; the same check is made against the PostgreSQL
/// catalogue in the parity test below, which is also what fails if a table is ever added to one
/// backend's schema and not the other's.
#[tokio::test]
async fn the_copy_lists_exactly_the_tables_the_schema_has() {
    let dir = tempfile::tempdir().unwrap();
    let sqlite = schema(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display())).await;
    if let Some(failure) = unlisted_tables("SQLite", &sqlite) {
        panic!("{failure}\n\n`TABLES` in src/copy.rs has to name every one of them, and only \
                them, with each table after the ones its foreign keys point at");
    }
}

#[tokio::test]
async fn the_two_schemas_describe_the_same_tables_and_columns() {
    // `LOGB_TEST_DATABASE_URL` is the test harness's own variable, separate from the app's
    // `LOGB_DATABASE_URL`: it names a scratch PostgreSQL server this test may migrate into,
    // not the database an instance serves. A skipped test is not a passing test -- CI always
    // provides the URL, so this cannot be quietly skipped forever.
    let Some(server) = common::test_server_url() else {
        eprintln!("skipped: set LOGB_TEST_DATABASE_URL to a PostgreSQL server to run this");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sqlite = schema(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display())).await;
    // A scratch database of its own, like every other test, rather than migrating into the
    // server's own `postgres` database: this test compares a schema built from nothing, and a
    // database left behind by an earlier run would let a dropped column keep passing.
    let (_scratch, pg) = common::scratch_database_on(&server).await;
    let postgres = schema(&pg).await;

    let failures: Vec<String> = [
        diff("tables and columns", &sqlite.columns, &postgres.columns),
        diff("column nullability", &sqlite.nullable, &postgres.nullable),
        diff("column types", &sqlite.types, &postgres.types),
        diff("indexes", &sqlite.indexes, &postgres.indexes),
        // Against the catalogue of the backend `--copy-to` exists to move data into, rather
        // than only inferring it from the SQLite check above and the tables diff.
        unlisted_tables("PostgreSQL", &postgres),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert!(failures.is_empty(), "schemas disagree.\n\n{}", failures.join("\n\n"));
}
