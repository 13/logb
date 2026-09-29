//! Every integration test, in one binary.
//!
//! One crate rather than a binary per file: each binary links all of logb and its
//! dependencies, so thirty-odd of them cost minutes of linking and tens of gigabytes of
//! `target/` per build. A theme is a module here, the way `sync` already was.
//!
//! The exception is `tests/timezone`: its tests change the process-wide instance timezone, which
//! every test here assumes is UTC, so they keep a process of their own.
mod common;

mod activities;
mod api_tokens;
mod attachments;
mod auth;
mod backup;
mod charging;
mod concurrency;
mod copy;
mod database_api;
mod dialect;
mod export;
mod harness;
mod health;
mod insights;
mod last_done;
mod migration_object_types;
mod notify;
mod object_list;
mod objects;
mod offline_edits;
mod openapi;
mod own_types;
mod pairing;
mod personal_preferences;
mod pointer;
mod pool;
mod reading_reminders;
mod reminders;
mod schema_parity;
mod search;
mod spa;
mod stats;
mod tags;
mod trips;
mod usage;
mod users;
mod weight;
mod sync;
