//! The places the two databases cannot be written the same way.
//!
//! Everything else in this app is portable SQL. Keeping the exceptions in one file means the
//! next person can read the whole surface of the difference in under a minute, rather than
//! discovering it one failing query at a time.
//!
//! There are eight, and items 2 to 4, 7 and 8 need code here:
//!
//! 1. Case-insensitive matching. Neither backend folds accents in SQL (SQLite's `LIKE` is
//!    ASCII-only; PostgreSQL's `ILIKE` follows the cluster collation), so search folds in
//!    Rust with `domain::tags::fold`, stores the result (`search_text`), and asks SQL only
//!    whether one already-folded string contains another -- item 7.
//! 2. Case-insensitive sorting of names -- `name_order`.
//! 3. How a transaction that intends to write begins -- `begin_write`.
//! 4. How a write transaction claims the right to be the only one -- `write_lock`. SQLite
//!    needs nothing in SQL: `BEGIN IMMEDIATE` takes the one write lock the database has, and
//!    writers queue for a single connection in `db::connect_writer` before they ever reach it.
//!    PostgreSQL permits concurrent writers and so has to be told not to. Only one backend's
//!    queue has a deadline: SQLite's writer waits at most `db::WRITE_WAIT` before a caller is
//!    answered 503; PostgreSQL's `pg_advisory_xact_lock` waits for its turn with no timeout at
//!    all.
//! 5. Case-insensitive username uniqueness. SQLite declares `UNIQUE COLLATE NOCASE` on the
//!    column; PostgreSQL has no per-column collation of that kind without `citext`, so its
//!    schema carries a unique index on `lower(username)` instead. The statements themselves
//!    need no adapter: every username lookup compares `lower(username) = lower($1)`, which is
//!    the same answer on SQLite (usernames are ASCII by `validate_username`) and is what makes
//!    PostgreSQL use that index rather than scan.
//! 6. Seeding. The SQLite migrations wrote the `currency` and `sync_epoch` settings rows with
//!    `INSERT ... randomblob()`; the PostgreSQL schema seeds nothing, and `db::seed_settings`
//!    now writes both on first start on either backend.
//! 7. "Does this column contain this string" -- `contains`. The same question, spelled so each
//!    backend answers it exactly and as fast as it can.
//! 8. A statement with thousands of bind parameters -- `values_list`. SQLite treats `$1` as a
//!    *named* parameter and looks every name up by a linear scan, so a long list is quadratic;
//!    it is spelled with anonymous `?` there.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

impl Backend {
    /// Which backend a connection URL names. Everything that is not SQLite is PostgreSQL:
    /// those are the only two drivers `db::connect` installs, so a third would fail to
    /// connect long before it reached a statement built here.
    pub fn of(url: &str) -> Self {
        if url.starts_with("sqlite:") { Self::Sqlite } else { Self::Postgres }
    }

    /// How to begin a transaction that is going to write.
    ///
    /// SQLite needs `BEGIN IMMEDIATE`: the default deferred begin takes its read snapshot
    /// first and only asks for the write lock at its first write, so under WAL two devices
    /// pushing at once can find the database changed underneath them and get
    /// `SQLITE_BUSY_SNAPSHOT` -- which `busy_timeout` does not retry, because waiting cannot
    /// fix a stale snapshot. Taking the lock up front puts the wait somewhere `busy_timeout`
    /// applies.
    ///
    /// PostgreSQL has no such statement and does not need one: readers never block writers, a
    /// write takes its row locks as it goes, and a conflict surfaces as a serialization error
    /// to retry rather than a stale snapshot. `BEGIN IMMEDIATE` there is a syntax error, which
    /// is how this was found -- every sync push answered 500.
    pub fn begin_write(&self) -> &'static str {
        match self {
            Self::Sqlite => "BEGIN IMMEDIATE",
            Self::Postgres => "BEGIN",
        }
    }

    /// How a write transaction claims the right to be the only one.
    ///
    /// SQLite needs nothing here: `BEGIN IMMEDIATE` already took the database's write lock,
    /// there is exactly one, and `db::connect_writer` has already made this process take its
    /// writers one at a time. PostgreSQL permits concurrent writers, which is precisely what
    /// this codebase is not written for -- the audit in part two's spec found five places where
    /// check-then-act is atomic only because SQLite serialises writers.
    ///
    /// The key is arbitrary but must never change: it names this application's write lock, and
    /// a different value would let two versions of LogB write concurrently against one
    /// database. `pg_advisory_xact_lock` releases at commit or rollback, including a rollback
    /// nobody wrote -- a dropped transaction, a panic, a killed connection -- which is why it
    /// is the transaction-scoped form rather than the session one.
    pub fn write_lock(&self) -> Option<&'static str> {
        match self {
            Self::Sqlite => None,
            Self::Postgres => Some("SELECT pg_advisory_xact_lock(4479001)"),
        }
    }

    /// Which backend an open connection talks to, for code that holds a connection rather than
    /// the `App` that knows it.
    pub fn of_connection(conn: &sqlx::AnyConnection) -> Self {
        if conn.backend_name() == <sqlx::Sqlite as sqlx::Database>::NAME {
            Self::Sqlite
        } else {
            Self::Postgres
        }
    }

    /// `VALUES`'s rows for a multi-row `INSERT`: `rows` rows of `width` parameters each, bound
    /// in order, row by row. The statement must have no other parameters.
    ///
    /// PostgreSQL numbers them `$1, $2, ...`, as every other statement in this app does.
    /// SQLite gets anonymous `?`: to it `$N` is a *named* parameter, and both SQLite's parser
    /// (resolving each name) and sqlx's binding (`sqlite3_bind_parameter_name`, per parameter,
    /// per execution) find a name by scanning the statement's whole list of them. That is
    /// quadratic in the number of parameters -- about a second for one 32 000-parameter
    /// statement, which made a batched import seven times slower than the per-row one it
    /// replaced. An anonymous `?` has no name to look up and is bound by position.
    pub fn values_list(&self, rows: usize, width: usize) -> String {
        let row = |r: usize| -> String {
            let params: Vec<String> = (1..=width)
                .map(|c| match self {
                    Self::Sqlite => "?".to_string(),
                    Self::Postgres => format!("${}", r * width + c),
                })
                .collect();
            format!("({})", params.join(", "))
        };
        (0..rows).map(row).collect::<Vec<_>>().join(", ")
    }

    /// SQLite sorts with `COLLATE NOCASE`; PostgreSQL sorts by `lower(...)`.
    ///
    /// Both fold only ASCII -- SQLite's `NOCASE` by definition, PostgreSQL's `lower` by the
    /// database collation -- but for ordering that agrees on every name a household is likely
    /// to type, and it is what keeps "Banana" out from in front of "apple".
    pub fn name_order(&self, column: &str) -> String {
        match self {
            Self::Sqlite => format!("{column} COLLATE NOCASE"),
            Self::Postgres => format!("lower({column})"),
        }
    }

    /// A predicate: `column` contains the text bound at `param`, character for character --
    /// both sides are already folded by `domain::tags::fold`, so no case rule may apply on top.
    /// The value bound must come from `contains_param`.
    ///
    /// SQLite uses `instr`, an exact substring test. `LIKE` would be wrong twice over there: it
    /// folds ASCII case on its own, and it refuses a pattern longer than
    /// `SQLITE_MAX_LIKE_PATTERN_LENGTH` (50 000 bytes) with an error, where a long term should
    /// simply find nothing. No index serves either spelling, and none is needed: the scan is
    /// over one narrow column of one user's rows.
    ///
    /// PostgreSQL uses `LIKE` with the term's wildcards escaped: its `LIKE` is case-sensitive
    /// and has no pattern limit, and it is the spelling a `pg_trgm` index can serve, which
    /// `strpos` is not.
    pub fn contains(&self, column: &str, param: &str) -> String {
        match self {
            Self::Sqlite => format!("instr({column}, {param}) > 0"),
            Self::Postgres => format!("{column} LIKE {param} ESCAPE '\\'"),
        }
    }

    /// The value to bind for `contains`'s parameter: the term itself on SQLite, and on
    /// PostgreSQL a `LIKE` pattern with `\`, `%` and `_` escaped so each matches only itself.
    pub fn contains_param(&self, folded: &str) -> String {
        match self {
            Self::Sqlite => folded.to_string(),
            Self::Postgres => {
                let mut pattern = String::with_capacity(folded.len() + 2);
                pattern.push('%');
                for c in folded.chars() {
                    if matches!(c, '\\' | '%' | '_') {
                        pattern.push('\\');
                    }
                    pattern.push(c);
                }
                pattern.push('%');
                pattern
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Backend;

    #[test]
    fn a_sqlite_url_is_sqlite_and_everything_else_is_postgres() {
        assert_eq!(Backend::of("sqlite:///data/logb.db?mode=rwc"), Backend::Sqlite);
        assert_eq!(Backend::of("sqlite::memory:"), Backend::Sqlite);
        assert_eq!(Backend::of("postgres://u:p@host/logb"), Backend::Postgres);
        assert_eq!(Backend::of("postgresql://u:p@host/logb"), Backend::Postgres);
    }

    #[test]
    fn each_backend_sorts_names_its_own_way() {
        assert_eq!(Backend::Sqlite.name_order("name"), "name COLLATE NOCASE");
        assert_eq!(Backend::Postgres.name_order("name"), "lower(name)");
        // Qualified columns are passed through whole, since the search joins two tables.
        assert_eq!(Backend::Postgres.name_order("o.name"), "lower(o.name)");
    }

    #[test]
    fn each_backend_asks_contains_its_own_way() {
        assert_eq!(Backend::Sqlite.contains("t", "$2"), "instr(t, $2) > 0");
        assert_eq!(Backend::Postgres.contains("t", "$2"), "t LIKE $2 ESCAPE '\\'");
        assert_eq!(Backend::Sqlite.contains_param("50%_a\\b"), "50%_a\\b");
        assert_eq!(Backend::Postgres.contains_param("50%_a\\b"), "%50\\%\\_a\\\\b%");
    }

    #[test]
    fn each_backend_spells_a_long_values_list_its_own_way() {
        assert_eq!(Backend::Postgres.values_list(2, 3), "($1, $2, $3), ($4, $5, $6)");
        assert_eq!(Backend::Sqlite.values_list(2, 3), "(?, ?, ?), (?, ?, ?)");
    }

    /// `BEGIN IMMEDIATE` is SQLite's spelling and PostgreSQL rejects it outright, so this is
    /// not a tuning knob: the wrong one there fails every writing transaction.
    #[test]
    fn each_backend_begins_a_writing_transaction_its_own_way() {
        assert_eq!(Backend::Sqlite.begin_write(), "BEGIN IMMEDIATE");
        assert_eq!(Backend::Postgres.begin_write(), "BEGIN");
    }

    /// The advisory key is load-bearing rather than decorative: two LogB builds that disagreed
    /// about it would each believe they held the write lock while writing concurrently, so it
    /// is pinned here and must never be changed.
    #[test]
    fn only_postgres_has_to_be_told_to_write_alone() {
        assert_eq!(Backend::Sqlite.write_lock(), None);
        assert_eq!(Backend::Postgres.write_lock(), Some("SELECT pg_advisory_xact_lock(4479001)"));
    }
}
