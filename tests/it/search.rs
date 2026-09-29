use crate::common;
use serde_json::json;

async fn seed(app: &common::TestApp) -> (i64, i64) {
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let bike = app.create_object(&app.client, "Cube Bike", None).await;
    let (car_id, bike_id) = (car["id"].as_i64().unwrap(), bike["id"].as_i64().unwrap());
    app.client.post(app.url(&format!("/objects/{car_id}/activities")))
        .json(&json!({ "date": "2024-03-01", "category": "maintenance", "title": "Oil change", "notes": "Castrol 5W-30" }))
        .send().await.unwrap();
    app.client.post(app.url(&format!("/objects/{bike_id}/activities")))
        .json(&json!({ "date": "2024-04-01", "category": "repair", "title": "New chain", "notes": "worn out" }))
        .send().await.unwrap();
    (car_id, bike_id)
}

#[tokio::test]
async fn finds_objects_and_activities() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (car_id, _) = seed(&app).await;

    // Matches an activity title, not an object.
    let r: serde_json::Value = app.client.get(app.url("/search?q=oil")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "{r}");
    let acts = r["activities"].as_array().unwrap();
    assert_eq!(acts.len(), 1, "{r}");
    assert_eq!(acts[0]["title"], "Oil change");
    assert_eq!(acts[0]["object_id"], car_id);
    assert_eq!(acts[0]["object_name"], "Golf", "an activity hit carries its object's name");

    // Matches an object name, case-insensitively.
    let r: serde_json::Value = app.client.get(app.url("/search?q=GOLF")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["objects"][0]["name"], "Golf");

    // Matches activity notes.
    let r: serde_json::Value = app.client.get(app.url("/search?q=castrol")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["activities"].as_array().unwrap().len(), 1, "{r}");

    // No hits is an empty result, not an error.
    let r: serde_json::Value = app.client.get(app.url("/search?q=zzzz")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0);
    assert_eq!(r["activities"].as_array().unwrap().len(), 0);
}

/// `type` holds an identifier -- `car`, `e_bike`, `other` -- not words anyone typed, and search
/// used to match it. That made a search box that answered questions about the schema: a German
/// user searching "Auto" found nothing while "car" found their Golf, "other" returned every
/// unclassified object at once, and "bike" dragged in every e-bike alongside the bicycles.
///
/// Unmapped legacy text was preserved into the description by the migration, so an object whose
/// old free-text category meant something to its owner is still found by those words.
#[tokio::test]
async fn objects_are_found_by_their_words_not_their_type() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let golf = app.create_object(&app.client, "Golf", Some("km")).await;
    assert_eq!(golf["type"], "car", "the fixture files this one as a car");

    let r: serde_json::Value = app.client.get(app.url("/search?q=car")).send().await.unwrap().json().await.unwrap();
    assert_eq!(
        r["objects"].as_array().unwrap().len(), 0,
        "`car` is a stored identifier, not something the user typed: {r}",
    );

    let r: serde_json::Value = app.client.get(app.url("/search?q=Golf")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "the name the user chose still finds it: {r}");
    assert_eq!(r["objects"][0]["id"], golf["id"]);

    // What the migration preserved is what a legacy owner will search for: an object whose old
    // free-text category did not map carries those words in its description.
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Odd one", "type": "other", "counter_unit": null,
                       "description": "Gravelbike Custom", "purchase_date": null,
                       "purchase_price_cents": null }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let r: serde_json::Value = app.client.get(app.url("/search?q=gravelbike")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "text kept in the description stays findable: {r}");

    // And the type of the object that carries it is still not a search term.
    let r: serde_json::Value = app.client.get(app.url("/search?q=other")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "`other` must not return every unclassified object: {r}");
}

#[tokio::test]
async fn wildcards_in_the_query_are_literal() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;
    // `%` would match everything if it reached LIKE unescaped.
    let r: serde_json::Value = app.client.get(app.url("/search?q=%25")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 0, "{r}");
}

#[tokio::test]
async fn search_is_scoped_to_the_caller() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;
    let eve = app.create_user_client("eve", "password123").await;
    let r: serde_json::Value = eve.get(app.url("/search?q=golf")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["objects"].as_array().unwrap().len(), 0, "another user's objects must not surface: {r}");
    assert_eq!(r["activities"].as_array().unwrap().len(), 0, "{r}");
}

#[tokio::test]
async fn blank_and_anonymous_queries_are_refused() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert_eq!(app.client.get(app.url("/search?q=%20")).send().await.unwrap().status(), 400);
    assert_eq!(app.client.get(app.url("/search")).send().await.unwrap().status(), 400);
    assert_eq!(common::new_client().get(app.url("/search?q=golf")).send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn search_paginates_without_duplicates() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Pagination object", None).await;
    for n in 0..30 {
        app.create_activity(&object["id"], &format!("pagination-{n}")).await;
    }
    let first: serde_json::Value = app.client.get(app.url("/search?q=pagination&limit=10&offset=0")).send().await.unwrap().json().await.unwrap();
    let second: serde_json::Value = app.client.get(app.url("/search?q=pagination&limit=10&offset=10")).send().await.unwrap().json().await.unwrap();
    assert_eq!(first["activities"].as_array().unwrap().len(), 10);
    assert_eq!(second["activities"].as_array().unwrap().len(), 10);
    assert!(first["has_more"].as_bool().unwrap());
    let ids: std::collections::HashSet<_> = first["activities"].as_array().unwrap().iter().chain(second["activities"].as_array().unwrap().iter()).map(|a| a["id"].as_i64().unwrap()).collect();
    assert_eq!(ids.len(), 20);
}

/// An object hit says which object it sits inside, so a list of four things called "Filter"
/// can be told apart without opening any of them.
#[tokio::test]
async fn a_search_hit_names_its_parent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light unique term", None).await;
    app.client.patch(app.url(&format!("/objects/{}", light["id"])))
        .json(&json!({ "name": "Main light unique term", "type": "other",
            "counter_unit": null, "description": "", "purchase_date": null,
            "purchase_price_cents": null, "parent_id": garage["id"] }))
        .send().await.unwrap();

    let res: serde_json::Value = app.client.get(app.url("/search?q=unique+term"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(res["objects"][0]["parent_name"], "Garage");
}

/// A root object has no parent to name, and says so with a null rather than an empty string:
/// the UI draws the line only when there is a parent.
#[tokio::test]
async fn a_root_objects_search_hit_has_no_parent_name() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Solo object distinctive", None).await;
    let res: serde_json::Value = app.client.get(app.url("/search?q=distinctive"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(res["objects"][0]["parent_name"], serde_json::Value::Null);
}

/// A trip's places are searched like its title and notes -- and the umlaut proves the fold
/// applies to them too.
#[tokio::test]
async fn a_trip_is_found_by_its_places() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let id = bike["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-06-01", "category": "trip", "title": "", "notes": "",
        "start_counter": 400, "counter_value": 600,
        "from_place": "Bäckerei Müller", "to_place": "Home"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    let r: serde_json::Value = app.client.get(app.url("/search?q=B%C3%A4ckerei")).send().await.unwrap().json().await.unwrap();
    let acts = r["activities"].as_array().unwrap();
    assert_eq!(acts.len(), 1, "{r}");
    assert_eq!(acts[0]["category"], "trip");
}

/// A tag matches through the same fold as everything else, and a term made of JSON
/// punctuation matches no tag rather than every tagged row (the guarantee the old
/// `match_tags` special case gave).
#[tokio::test]
async fn tags_are_found_folded_and_punctuation_finds_nothing() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let id = bike["id"].as_i64().unwrap();
    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "name": "Tern", "type": "car", "description": "", "tags": ["Fahrräder"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let r = app.search("fahrrader").await;
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "{r}");
    for term in ["[", "\"", "[\"Fahrr"] {
        let r = app.search(term).await;
        assert_eq!(r["objects"].as_array().unwrap().len(), 0, "{term}: {r}");
        assert_eq!(r["activities"].as_array().unwrap().len(), 0, "{term}: {r}");
    }
}

// --- `search_text`: every write path keeps it, and the matching is the old in-Rust one. ---

/// The hits' ids, objects and activities, for a term -- the shape most of the tests below
/// assert on.
async fn hits(app: &common::TestApp, term: &str) -> (Vec<i64>, Vec<i64>) {
    let r = app.search(term).await;
    let ids = |key: &str| r[key].as_array().unwrap().iter().map(|h| h["id"].as_i64().unwrap()).collect();
    (ids("objects"), ids("activities"))
}

async fn uuid_of(app: &common::TestApp, table: &str, id: i64) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT client_uuid FROM {table} WHERE id = $1")))
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap()
}

/// A REST edit is searchable by its new words at once, and no longer by its old ones.
#[tokio::test]
async fn a_rest_edit_is_found_by_its_new_words() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Oldname", None).await;
    let oid = object["id"].as_i64().unwrap();
    let activity = app.create_activity(&object["id"], "Oldtitle").await;
    let aid = activity["id"].as_i64().unwrap();

    let res = app.client.patch(app.url(&format!("/objects/{oid}")))
        .json(&json!({ "name": "Newname", "type": "other", "description": "Freshwords", "tags": ["Gärten"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let res = app.client.patch(app.url(&format!("/activities/{aid}")))
        .json(&json!({ "date": "2024-03-01", "category": "maintenance", "title": "Newtitle",
                       "notes": "Castrol", "tags": ["Sommer"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    assert_eq!(hits(&app, "oldname").await.0, Vec::<i64>::new());
    assert_eq!(hits(&app, "oldtitle").await.1, Vec::<i64>::new());
    for term in ["newname", "freshwords", "garten"] {
        assert_eq!(hits(&app, term).await.0, vec![oid], "{term}");
    }
    for term in ["newtitle", "castrol", "sommer"] {
        assert_eq!(hits(&app, term).await.1, vec![aid], "{term}");
    }
}

/// A synced `set` of any searched field -- the other door into the same columns.
#[tokio::test]
async fn a_synced_edit_is_found_by_its_new_words() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Tern", Some("km")).await;
    let oid = object["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{oid}/activities"))).json(&json!({
        "date": "2026-06-01", "category": "trip", "title": "Ride", "notes": "",
        "start_counter": 400, "counter_value": 600, "from_place": "Home", "to_place": "Work"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let aid = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();
    let (ouuid, auuid) = (uuid_of(&app, "objects", oid).await, uuid_of(&app, "activities", aid).await);

    let edited_at = (chrono::Utc::now() + chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let op = |n: usize, entity: &str, uuid: &str, field: &str, value: serde_json::Value| json!({
        "client_op_id": format!("search-op-{n}"), "entity": entity, "entity_uuid": uuid,
        "op": "set", "field": field, "value": value, "edited_at": edited_at, "device_id": "phone"
    });
    let ops = json!({ "ops": [
        op(1, "object", &ouuid, "name", json!("Syncedname")),
        op(2, "object", &ouuid, "description", json!("Syncdesc")),
        op(3, "object", &ouuid, "tags", json!("[\"Syncobjtag\"]")),
        op(4, "activity", &auuid, "title", json!("Synctitle")),
        op(5, "activity", &auuid, "notes", json!("Syncnotes")),
        op(6, "activity", &auuid, "tags", json!("[\"Syncacttag\"]")),
        op(7, "activity", &auuid, "from_place", json!("Café Zürich")),
        op(8, "activity", &auuid, "to_place", json!("Syncto")),
    ]});
    let res = app.push_raw(&ops).await;
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    for result in body["results"].as_array().unwrap() {
        assert_eq!(result["outcome"], "accepted", "{body}");
    }

    assert_eq!(hits(&app, "tern").await.0, Vec::<i64>::new(), "the old name is gone");
    for term in ["syncedname", "syncdesc", "syncobjtag"] {
        assert_eq!(hits(&app, term).await.0, vec![oid], "{term}");
    }
    assert_eq!(hits(&app, "ride").await.1, Vec::<i64>::new(), "the old title is gone");
    for term in ["synctitle", "syncnotes", "syncacttag", "cafe zurich", "syncto"] {
        assert_eq!(hits(&app, term).await.1, vec![aid], "{term}");
    }
}

/// An import writes its rows in bulk, and every one of them is searchable afterwards.
#[tokio::test]
async fn imported_rows_are_found() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Importable Rad", None).await;
    let res = app.client.post(app.url(&format!("/objects/{}/activities", object["id"])))
        .json(&json!({ "date": "2024-03-01", "category": "repair", "title": "Schlauch",
                       "notes": "Pannenhilfe", "tags": ["Fahrräder"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let archive = app.client.get(app.url("/export")).send().await.unwrap().bytes().await.unwrap();

    let anna = app.create_user_client("anna", "password123").await;
    let res = anna.post(app.url("/import")).header("content-type", "application/zip")
        .body(archive.to_vec()).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let unfolded: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM objects WHERE search_text IS NULL) \
              + (SELECT count(*) FROM activities WHERE search_text IS NULL)")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(unfolded, 0, "an import must not leave a row search cannot see");
    for term in ["importable", "pannenhilfe", "fahrrader"] {
        let r: serde_json::Value = anna.get(app.url(&format!("/search?q={term}")))
            .send().await.unwrap().json().await.unwrap();
        let found = r["objects"].as_array().unwrap().len() + r["activities"].as_array().unwrap().len();
        assert_eq!(found, 1, "{term}: {r}");
    }
}

/// Rows with no `search_text` -- every row of a database upgraded from before the column --
/// are folded at the next start, and a change of folding refolds every row.
#[tokio::test]
async fn unfolded_rows_are_folded_at_the_next_start() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Ölkanne", None).await;
    app.create_activity(&object["id"], "Élan vital").await;
    let other = app.create_object(&app.client, "Stale", None).await;
    // As a database upgraded from before the column looks, plus one row holding text an older
    // folding produced -- which the version bump has to throw away rather than trust.
    for sql in [
        "UPDATE objects SET search_text = NULL WHERE name <> 'Stale'",
        "UPDATE activities SET search_text = NULL",
        "UPDATE objects SET search_text = 'something else entirely' WHERE name = 'Stale'",
        "UPDATE settings SET value = '0' WHERE key = 'search_fold_version'",
    ] {
        sqlx::query(sql).execute(&app.state.db).await.unwrap();
    }
    app.release_database().await;

    let again = common::spawn_on(&app.database_url()).await;
    let res = again.login(&again.client, "ben", "correct horse").await;
    assert_eq!(res.status(), 200);
    assert_eq!(hits(&again, "olkanne").await.0, vec![object["id"].as_i64().unwrap()]);
    assert_eq!(hits(&again, "elan").await.1.len(), 1);
    assert_eq!(hits(&again, "stale").await.0, vec![other["id"].as_i64().unwrap()]);
    assert_eq!(hits(&again, "entirely").await.0, Vec::<i64>::new());
    let version: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'search_fold_version'")
        .fetch_one(&again.state.db).await.unwrap();
    assert_eq!(version, logb::search_text::FOLD_VERSION);
}

/// Case and accents fold on both sides: the stored text and the term.
#[tokio::test]
async fn case_and_accents_fold_on_both_sides() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let a = app.create_object(&app.client, "ÖLWECHSEL Crème", None).await;
    let b = app.create_object(&app.client, "olwechsel creme", None).await;
    app.create_object(&app.client, "Oil change", None).await;
    let mut both = vec![a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap()];
    both.sort();
    for term in ["Ölwechsel", "OLWECHSEL", "ölWechsel", "CRÈME", "creme", "Cre\u{300}me"] {
        let mut found = hits(&app, term).await.0;
        found.sort();
        assert_eq!(found, both, "{term}");
    }
}

/// A term of several words is one phrase, as it always was: it matches where the words stand
/// together in that order within one field, not wherever each word appears.
#[tokio::test]
async fn a_multi_word_term_is_one_phrase_within_one_field() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Alpha Beta", "type": "other", "description": "Gamma" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();
    assert_eq!(hits(&app, "alpha beta").await.0, vec![id]);
    assert_eq!(hits(&app, "beta alpha").await.0, Vec::<i64>::new());
    // The name ends where the description begins; no hit may straddle the two.
    assert_eq!(hits(&app, "beta gamma").await.0, Vec::<i64>::new());
    assert_eq!(hits(&app, "betagamma").await.0, Vec::<i64>::new());
    assert_eq!(hits(&app, "beta\u{1f}gamma").await.0, Vec::<i64>::new());
}

/// `_` and `\` are literal too, not only `%`: each finds only the rows that contain it.
#[tokio::test]
async fn underscore_and_backslash_are_literal() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let under = app.create_object(&app.client, "Box_1", None).await;
    let slash = app.create_object(&app.client, "C:\\Temp 50%", None).await;
    app.create_object(&app.client, "Boxa1 C-Temp 50", None).await;
    assert_eq!(hits(&app, "_").await.0, vec![under["id"].as_i64().unwrap()]);
    assert_eq!(hits(&app, "box_1").await.0, vec![under["id"].as_i64().unwrap()]);
    assert_eq!(hits(&app, "\\").await.0, vec![slash["id"].as_i64().unwrap()]);
    assert_eq!(hits(&app, "c:\\temp").await.0, vec![slash["id"].as_i64().unwrap()]);
    assert_eq!(hits(&app, "50%").await.0, vec![slash["id"].as_i64().unwrap()]);
}

/// A deleted object or activity is never a hit, nor is a live activity of a deleted object.
#[tokio::test]
async fn deleted_rows_are_not_found() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let keep = app.create_object(&app.client, "Keeper vanish", None).await;
    let gone = app.create_object(&app.client, "Gone vanish", None).await;
    let doomed = app.create_activity(&keep["id"], "vanish one").await;
    let kept = app.create_activity(&keep["id"], "vanish two").await;
    app.create_activity(&gone["id"], "vanish three").await;
    app.delete_object(&gone).await;
    let res = app.client.delete(app.url(&format!("/activities/{}", doomed["id"]))).send().await.unwrap();
    assert!(res.status().is_success(), "{}", res.status());

    let (objects, activities) = hits(&app, "vanish").await;
    assert_eq!(objects, vec![keep["id"].as_i64().unwrap()]);
    assert_eq!(activities, vec![kept["id"].as_i64().unwrap()]);
}

/// Paging runs in SQL now, and has to answer what the in-Rust paging did: each list pages on
/// its own, in its own order, and `has_more` is true while either has more.
#[tokio::test]
async fn pages_keep_their_order_and_has_more() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let mut ids = Vec::new();
    for name in ["Pagey c", "pagey A", "Pagey b"] {
        ids.push(app.create_object(&app.client, name, None).await["id"].as_i64().unwrap());
    }
    let archived = app.create_object(&app.client, "Pagey 0 archived", None).await;
    let res = app.client.patch(app.url(&format!("/objects/{}", archived["id"])))
        .json(&json!({ "name": "Pagey 0 archived", "type": "other", "description": "", "archived": true }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let page = |offset: i64| {
        let app = &app;
        async move {
            let r: serde_json::Value = app.client
                .get(app.url(&format!("/search?q=pagey&limit=2&offset={offset}")))
                .send().await.unwrap().json().await.unwrap();
            let ids: Vec<i64> = r["objects"].as_array().unwrap().iter().map(|o| o["id"].as_i64().unwrap()).collect();
            (ids, r["has_more"].as_bool().unwrap())
        }
    };
    // Unarchived first, by name case-insensitively; the archived one last.
    assert_eq!(page(0).await, (vec![ids[1], ids[2]], true));
    assert_eq!(page(2).await, (vec![ids[0], archived["id"].as_i64().unwrap()], false));
    assert_eq!(page(4).await, (vec![], false));
}
