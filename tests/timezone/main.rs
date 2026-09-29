//! Tests that change the process-wide instance timezone (`db::set_timezone`).
//!
//! Everything else is in `tests/it`, one binary whose tests assume the instance runs on UTC;
//! these get a process of their own so that assumption holds there.
#[path = "../it/common/mod.rs"]
mod common;

mod backup_day;
mod notifications;
