//! The sync protocol's integration tests, split by theme. Shared fixtures live in
//! `helpers`; everything else is a pure move from the former `tests/sync.rs`.
use crate::common;
mod helpers;

mod bootstrap;
mod cascade;
mod deleted;
mod energy;
mod pull;
mod purge;
mod push;
mod read_paths;
mod rest;
mod tags_types;
mod trips;
mod validation;
