use super::common;
use super::helpers::*;
use serde_json::json;


/// The stored value of one field, read straight from the row rather than through a REST read
/// path that might apply its own transformation -- the question here is what the column holds.
async fn stored_field(db: &sqlx::AnyPool, table: &str, uuid: &str, field: &str) -> Option<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {field} FROM {table} WHERE client_uuid = $1"
    )))
    .bind(uuid)
    .fetch_one(db)
    .await
    .unwrap()
}

/// The `field_clock` row for one entity/field, if any.
async fn field_clock_of(
    db: &sqlx::AnyPool,
    entity: &str,
    uuid: &str,
    field: &str,
) -> Option<(String, String)> {
    sqlx::query_as(
        "SELECT edited_at, device_id FROM field_clock WHERE entity = $1 AND entity_uuid = $2 AND field = $3")
        .bind(entity)
        .bind(uuid)
        .bind(field)
        .fetch_optional(db)
        .await
        .unwrap()
}

/// Pushes a `set` on `field` at a row that is already tombstoned, and pins the whole rejection
/// contract: the outcome and reason, the stored value untouched, and no trace left in either
/// `field_clock` or `changes` -- a rejected op has to look as if it never arrived, the same
/// promise `an_operation_naming_the_removed_field_is_rejected_not_fatal` and
/// `a_field_outside_the_whitelist_is_rejected` pin for the other rejection reasons.
///
/// `edited_at` is far in the future and `device_id` differs from whatever created the row, so a
/// last-write-wins comparison alone (if the deleted check were skipped or misplaced after the
/// `field_clock` read) would let this op win and overwrite the stored value -- the rejection has
/// to come from the row being deleted, not from losing on the clock.
async fn assert_set_on_deleted_row_is_rejected(
    app: &common::TestApp,
    entity: &str,
    table: &str,
    uuid: &str,
    field: &str,
) {
    let before_value = stored_field(&app.state.db, table, uuid, field).await;
    let before_clock = field_clock_of(&app.state.db, entity, uuid, field).await;
    // Every caller below names a field from the entity's own whitelist on a row that was just
    // created over REST, and `record::record_create` stamps `field_clock` for *every*
    // whitelisted field at creation (see its doc comment) -- so a clock row must already exist
    // here. Asserting that is what makes the "unchanged" comparison below mean something: an
    // absent-before-and-after clock would equal itself trivially and prove nothing about the
    // rejected op leaving `field_clock` alone.
    assert!(
        before_clock.is_some(),
        "{entity}: field_clock must already hold a row for {field}, stamped by record_create"
    );
    let op_id = format!("op-deleted-{entity}");

    let body: serde_json::Value = app
        .push_raw(&push_body(json!([{
            "client_op_id": op_id, "entity": entity, "entity_uuid": uuid,
            "op": "set", "field": field, "value": "should never land",
            "edited_at": after_now(31_536_000), "device_id": "intruder"
        }])))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        body["results"][0]["outcome"], "rejected",
        "{entity}: {body}"
    );
    assert_eq!(
        body["results"][0]["reason"], "this item was deleted",
        "{entity}: {body}"
    );

    assert_eq!(
        stored_field(&app.state.db, table, uuid, field).await,
        before_value,
        "{entity}: the stored value must not change"
    );
    assert_eq!(
        field_clock_of(&app.state.db, entity, uuid, field).await,
        before_clock,
        "{entity}: a rejected op must not advance field_clock"
    );
    assert_eq!(
        app.count_changes_of(&op_id).await,
        0,
        "{entity}: a rejected op must leave no changes row"
    );
}

/// A `set` pushed at a row that was deleted -- over REST or over sync, whichever created it --
/// is rejected with a reason naming the deletion, for every entity a `set` op can name. Without
/// the check this task adds, the write would fall straight through to `field_clock`/`wins` and
/// silently resurrect content on a row the user removed, without ever un-deleting the row
/// itself: the object (say) would stay invisible everywhere while one of its columns quietly
/// changed underneath it.
#[tokio::test]
async fn a_set_on_a_deleted_row_is_rejected_for_every_entity() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_uuid = client_uuid(&app.state.db, "objects", car["id"].as_i64().unwrap()).await;
    app.delete_object(&car).await;
    assert_set_on_deleted_row_is_rejected(&app, "object", "objects", &object_uuid, "name").await;

    // A live parent object for the three child entities below -- deleting each child on its
    // own, never the parent, so the parent's own aliveness cannot be what is under test.
    let (_, activity_id, reminder_id, attachment_id) =
        object_with_children(&app, &app.client, "Yaris").await;

    let activity_uuid = client_uuid(&app.state.db, "activities", activity_id).await;
    let res = app
        .client
        .delete(app.url(&format!("/activities/{activity_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        204,
        "delete activity: {}",
        res.text().await.unwrap()
    );
    assert_set_on_deleted_row_is_rejected(&app, "activity", "activities", &activity_uuid, "title")
        .await;

    let reminder_uuid = client_uuid(&app.state.db, "reminders", reminder_id).await;
    let res = app
        .client
        .delete(app.url(&format!("/reminders/{reminder_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        204,
        "delete reminder: {}",
        res.text().await.unwrap()
    );
    assert_set_on_deleted_row_is_rejected(&app, "reminder", "reminders", &reminder_uuid, "title")
        .await;

    let attachment_uuid = client_uuid(&app.state.db, "attachments", attachment_id).await;
    let res = app
        .client
        .delete(app.url(&format!("/attachments/{attachment_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        204,
        "delete attachment: {}",
        res.text().await.unwrap()
    );
    assert_set_on_deleted_row_is_rejected(
        &app,
        "attachment",
        "attachments",
        &attachment_uuid,
        "caption",
    )
    .await;

    let scooter = app.post_json("/types", &json!({
        "name": "E-scooter", "icon": "e-bike", "categories": ["repair"], "counter_unit": "km"
    })).await;
    let type_id = scooter["id"].as_i64().unwrap();
    let type_uuid = scooter["client_uuid"].as_str().unwrap().to_string();
    let res = app
        .client
        .delete(app.url(&format!("/types/{type_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        204,
        "delete type: {}",
        res.text().await.unwrap()
    );
    assert_set_on_deleted_row_is_rejected(&app, "object_type", "object_types", &type_uuid, "name")
        .await;
}

/// The deleted check has to run before every other `set` validation, or a deleted row's op
/// comes back with a reason that belongs to a completely different failure. Two ways that could
/// happen if the check were placed after even one of the checks it now precedes:
///
/// - An invalid VALUE: `validate_value` would answer "name is required" for an empty object
///   name, live row or not, if it ran first.
/// - A name collision on an unrelated row: `type_field`'s own name-taken check would answer
///   "you already have a type with this name" for a rename that happens to collide with some
///   OTHER, still-live type -- exactly the scenario a deleted type getting reused as a name
///   produces -- if it ran first.
///
/// Both are pinned here rather than only in `assert_set_on_deleted_row_is_rejected` above,
/// whose op value ("should never land") and field (each entity's own whitelisted text field)
/// are deliberately unremarkable -- they would pass every other check just fine, so that test
/// alone cannot tell "deleted checked first" apart from "deleted checked last, but nothing else
/// happened to object".
#[tokio::test]
async fn a_set_on_a_deleted_row_is_rejected_before_any_other_set_validation() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    // An invalid value on a deleted row.
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_uuid = client_uuid(&app.state.db, "objects", car["id"].as_i64().unwrap()).await;
    app.delete_object(&car).await;
    let body: serde_json::Value = app
        .push_raw(&push_body(json!([{
            "client_op_id": "op-invalid-on-deleted", "entity": "object", "entity_uuid": object_uuid,
            "op": "set", "field": "name", "value": "",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"], "this item was deleted",
        "an invalid value must not preempt the deleted check: {body}"
    );

    // A rename that collides with a DIFFERENT, live type's name.
    let old_type = app
        .post_json(
            "/types",
            &json!({
                "name": "Scooter", "icon": "e-bike", "categories": ["repair"], "counter_unit": "km"
            }),
        )
        .await;
    let old_id = old_type["id"].as_i64().unwrap();
    let old_uuid = old_type["client_uuid"].as_str().unwrap().to_string();
    let res = app
        .client
        .delete(app.url(&format!("/types/{old_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        204,
        "delete type: {}",
        res.text().await.unwrap()
    );
    // A live type takes the exact name the deleted one is about to be renamed to, after the
    // deletion -- so this is genuinely a name only a live row holds now, not a leftover
    // collision with the deleted row's own former name.
    app.post_json(
        "/types",
        &json!({
            "name": "Trailer", "icon": "car", "categories": ["repair"], "counter_unit": "km"
        }),
    )
    .await;
    let body: serde_json::Value = app
        .push_raw(&push_body(json!([{
            "client_op_id": "op-taken-on-deleted", "entity": "object_type", "entity_uuid": old_uuid,
            "op": "set", "field": "name", "value": "Trailer",
            "edited_at": after_now(60), "device_id": "phone"
        }])))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "{body}");
    assert_eq!(
        body["results"][0]["reason"], "this item was deleted",
        "a name collision on another row must not preempt the deleted check: {body}"
    );
}
