//! The JSON archive: its shape, and the routes that write and read one.

pub(super) fn default_weight_unit() -> String {
    "kg".into()
}
use crate::api::attachments::AttachmentOut;
use crate::error::AppError;
use crate::state::App;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod import;
mod write;

use import::import;
use write::export;


pub fn router(max_import_bytes: usize) -> Router<App> {
    Router::new()
        .route("/export", get(export))
        .route("/import", post(import))
        .layer(DefaultBodyLimit::max(max_import_bytes))
}

#[derive(Serialize, Deserialize)]
pub(super) struct AttachmentExport {
    pub(super) sha256: String,
    pub(super) original_name: String,
    pub(super) mime: String,
    pub(super) kind: String,
    pub(super) caption: String,
    pub(super) taken_at: Option<String>,
    pub(super) created_at: String,
}

#[derive(Serialize, Deserialize)]
pub(super) struct ActivityExport {
    pub(super) date: String,
    pub(super) category: String,
    pub(super) title: String,
    pub(super) notes: String,
    pub(super) counter_value: Option<i64>,
    pub(super) cost_cents: Option<i64>,
    #[serde(default)]
    pub(super) quantity_milli: Option<i64>,
    pub(super) created_at: String,
    pub(super) attachments: Vec<AttachmentExport>,
    // Added with tags; an older archive's entries are untagged.
    #[serde(default)]
    pub(super) tags: Vec<String>,
    // Added with trips; an older archive's entries carry none of the five, so every one of them
    // is `None` on import -- exactly what a non-trip entry already stores.
    #[serde(default)]
    pub(super) start_counter: Option<i64>,
    #[serde(default)]
    pub(super) from_place: Option<String>,
    #[serde(default)]
    pub(super) to_place: Option<String>,
    #[serde(default)]
    pub(super) duration_minutes: Option<i64>,
    #[serde(default)]
    pub(super) battery_used_pct: Option<i64>,
    // Added with charging: an older archive's entries carry no such key, and every one of them
    // was never a full charge -- exactly what a non-fuel entry already stores.
    #[serde(default)]
    pub(super) charged_full: i64,
    #[serde(default)]
    pub(super) weight_grams: Option<i64>,
    #[serde(default)]
    pub(super) fuel_level_pct: Option<i64>,
    #[serde(default)]
    pub(super) meter_reading_milli: Option<i64>,
    #[serde(default)]
    pub(super) period_start: Option<String>,
    #[serde(default)]
    pub(super) period_end: Option<String>,
    #[serde(default)]
    pub(super) estimated: i64,
    #[serde(default)]
    pub(super) meter_reset: i64,
}

#[derive(Serialize, Deserialize)]
pub(super) struct ReminderExport {
    pub(super) title: String,
    pub(super) notes: String,
    pub(super) due_date: Option<String>,
    pub(super) due_counter: Option<i64>,
    pub(super) repeat_months: Option<i64>,
    pub(super) repeat_counter: Option<i64>,
    pub(super) done_at: Option<String>,
    pub(super) done_activity_index: Option<usize>,
    pub(super) created_at: String,
    // Added after version 1 archives already existed in the wild; `#[serde(default)]` lets
    // those older archives import as reminders that simply were never snoozed.
    #[serde(default)]
    pub(super) snoozed_until: Option<String>,
    // Added with reading reminders; an older archive's reminders are all service reminders.
    #[serde(default = "crate::api::reminders::service_kind")]
    pub(super) kind: String,
    #[serde(default)]
    pub(super) every_n: Option<i64>,
    #[serde(default)]
    pub(super) every_unit: Option<String>,
    #[serde(default)]
    pub(super) schedule: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct ObjectExport {
    pub(super) name: String,
    /// Archives written before object types carry `category` instead. Both are optional (and
    /// omitted from an archive this app writes today, via `skip_serializing_if`) so one archive
    /// format does not become two structs; on import, exactly one is expected to be present --
    /// see `resolve_type`, which decides what happens otherwise.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub(super) type_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) category: Option<String>,
    pub(super) counter_unit: Option<String>,
    #[serde(default)]
    pub(super) fuel_unit: Option<String>,
    pub(super) description: String,
    pub(super) purchase_date: Option<String>,
    pub(super) purchase_price_cents: Option<i64>,
    pub(super) archived_at: Option<String>,
    pub(super) created_at: String,
    pub(super) cover_sha256: Option<String>,
    pub(super) activities: Vec<ActivityExport>,
    pub(super) attachments: Vec<AttachmentExport>,
    pub(super) reminders: Vec<ReminderExport>,
    // Added with tags; an older archive's objects are untagged.
    #[serde(default)]
    pub(super) tags: Vec<String>,
    // Added with charging; an older archive's objects carry no price.
    #[serde(default)]
    pub(super) energy_price_milli: Option<i64>,
    #[serde(default = "default_weight_unit")]
    pub(super) weight_unit: String,
    #[serde(default)]
    pub(super) fuel_capacity_milli: Option<i64>,
    #[serde(default)]
    pub(super) resource_unit: Option<String>,
    #[serde(default)]
    pub(super) resource_kind: Option<String>,
    #[serde(default)]
    pub(super) measurement_mode: Option<String>,
    #[serde(default)]
    pub(super) monthly_target_milli: Option<i64>,
    #[serde(default)]
    pub(super) low_level_pct: Option<i64>,
    #[serde(default)]
    pub(super) private: bool,
}

/// A user's own type. Objects keep `type` as `custom:<client_uuid>`, so the uuid is what ties
/// them together inside the archive.
#[derive(Serialize, Deserialize)]
pub(super) struct TypeExport {
    pub(super) client_uuid: String,
    pub(super) name: String,
    pub(super) icon: String,
    pub(super) categories: Vec<String>,
    pub(super) counter_unit: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Export {
    pub(super) version: u32,
    pub(super) exported_at: String,
    pub(super) currency: String,
    // Added with own types; an older archive has none.
    #[serde(default)]
    pub(super) types: Vec<TypeExport>,
    pub(super) objects: Vec<ObjectExport>,
}

/// The `files` row an attachment points at always belongs to the same user (attachments hang
/// off that user's objects), so a miss here means the two tables disagree. Report it as an
/// internal error rather than panicking on a bare `HashMap` index.
pub(super) fn sha_of(sha_by_file: &HashMap<i64, String>, file_id: i64) -> Result<String, AppError> {
    sha_by_file
        .get(&file_id)
        .cloned()
        .ok_or_else(|| AppError::Internal(format!("attachment references unknown file {file_id}")))
}

pub(super) fn att_export(
    a: &AttachmentOut,
    sha_by_file: &HashMap<i64, String>,
) -> Result<AttachmentExport, AppError> {
    Ok(AttachmentExport {
        sha256: sha_of(sha_by_file, a.file_id)?,
        original_name: a.original_name.clone(),
        mime: a.mime.clone(),
        kind: a.kind.clone(),
        caption: a.caption.clone(),
        taken_at: a.taken_at.clone(),
        created_at: a.created_at.clone(),
    })
}

#[derive(Deserialize)]
pub struct ExportQuery {
    pub object_id: Option<i64>,
    #[serde(default)]
    pub exclude_body: bool,
}

/// The app icon, taken from the embedded SPA build rather than a second copy in the tree.
///
/// Absent in a backend-only build (`frontend/dist/` empty, as in a plain `cargo test` before
/// the frontend has ever been built), which is not a reason to fail an export -- the archive is
/// simply without it, the same way a blob missing from disk is skipped below.
pub(super) fn icon_bytes() -> Option<Vec<u8>> {
    crate::spa::Assets::get("icon.svg").map(|f| f.data.into_owned())
}

