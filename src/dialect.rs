//! The places the two databases cannot be written the same way.
//!
//! Everything else in this app is portable SQL. Keeping the exceptions in one file means the
//! next person can read the whole surface of the difference in under a minute, rather than
//! discovering it one failing query at a time.
//!
//! There are four, and only the first three need code here:
//!
//! 1. Case-insensitive matching in `LIKE` -- `case_insensitive_like`.
//! 2. Case-insensitive sorting of names -- `name_order`.
//! 3. Case-insensitive username uniqueness. SQLite declares `UNIQUE COLLATE NOCASE` on the
//!    column; PostgreSQL has no per-column collation of that kind without `citext`, so its
//!    schema carries a unique index on `lower(username)` instead. The statements themselves
//!    need no adapter: every username lookup compares `lower(username) = lower($1)`, which is
//!    the same answer on SQLite (usernames are ASCII by `validate_username`) and is what makes
//!    PostgreSQL use that index rather than scan.
//! 4. Seeding. The SQLite migrations wrote the `currency` and `sync_epoch` settings rows with
//!    `INSERT ... randomblob()`; the PostgreSQL schema seeds nothing, and `db::seed_settings`
//!    now writes both on first start on either backend.

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

    /// SQLite's `LIKE` already ignores case for ASCII; PostgreSQL needs `ILIKE`.
    ///
    /// The two are not equivalent beyond ASCII, and that difference is real rather than
    /// cosmetic: `ILIKE` folds case by the server's collation, so on a UTF-8 PostgreSQL
    /// "ölwechsel" finds "Ölwechsel", while SQLite's built-in `LIKE` folds only the 26 ASCII
    /// letters and does not. Folding the rest would need ICU on SQLite's side. See
    /// `tests/dialect.rs`, which pins both halves of that.
    pub fn case_insensitive_like(&self) -> &'static str {
        match self {
            Self::Sqlite => "LIKE",
            Self::Postgres => "ILIKE",
        }
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
    fn each_backend_gets_the_operator_it_understands() {
        assert_eq!(Backend::Sqlite.case_insensitive_like(), "LIKE");
        assert_eq!(Backend::Postgres.case_insensitive_like(), "ILIKE");
        assert_eq!(Backend::Sqlite.name_order("name"), "name COLLATE NOCASE");
        assert_eq!(Backend::Postgres.name_order("name"), "lower(name)");
        // Qualified columns are passed through whole, since the search joins two tables.
        assert_eq!(Backend::Postgres.name_order("o.name"), "lower(o.name)");
    }
}
