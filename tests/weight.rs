mod common;
use serde_json::{json, Value};

async fn body(app: &common::TestApp) -> i64 {
    let response = app.client.post(app.url("/objects")).json(&json!({"name":"Body", "type":"body", "weight_unit":"lb"})).send().await.unwrap();
    assert_eq!(response.status(), 201);
    response.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}
async fn weight(app: &common::TestApp, id: i64, grams: i64, date: &str) -> Value {
    let response = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({"date":date,"category":"weight","title":"Weight","weight_grams":grams})).send().await.unwrap();
    assert_eq!(response.status(), 201, "{}", response.text().await.unwrap());
    response.json().await.unwrap()
}
async fn read(app: &common::TestApp, path: &str) -> Value {
    let response = app.client.get(app.url(path)).send().await.unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    response.json().await.unwrap()
}

#[tokio::test]
async fn latest_weight_uses_date_then_creation_and_deletion_reveals_previous() {
    let app = common::spawn().await; app.setup("ben", "correct horse").await;
    let id = body(&app).await;
    weight(&app, id, 85000, "2026-01-01").await;
    weight(&app, id, 82000, "2026-02-01").await;
    weight(&app, id, 95000, "2025-12-01").await; // backdated highest is not current
    let last = weight(&app, id, 81500, "2026-02-01").await;
    weight(&app, id, 99000, "2099-01-01").await; // future entry is not current
    let object = read(&app, &format!("/objects/{id}")).await;
    assert_eq!(object["weight_unit"], "lb");
    assert_eq!(object["stats"]["latest_weight_grams"], 81500);
    assert_eq!(object["stats"]["latest_weight_date"], "2026-02-01");
    assert!(object["stats"]["current_counter"].is_null());
    let history = read(&app, &format!("/objects/{id}/weight")).await;
    assert_eq!(history.as_array().unwrap().len(), 4);
    assert_eq!(history[0]["id"], last["id"]);
    let res = app.client.delete(app.url(&format!("/activities/{}", last["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(read(&app, &format!("/objects/{id}")).await["stats"]["latest_weight_grams"], 82000);
    assert_eq!(read(&app, "/objects").await[0]["stats"]["latest_weight_grams"], 82000);
}

#[tokio::test]
async fn weight_validation_edits_replay_and_sync_use_the_same_rules() {
    let app = common::spawn().await; app.setup("ben", "correct horse").await;
    let id = body(&app).await;
    for data in [
        json!({"weight_grams":0}), json!({"weight_grams":-1}), json!({"weight_grams":70.5}),
        json!({"weight_grams":null}), json!({"weight_grams":1000000001}),
        json!({"weight_grams":70000,"counter_value":1}), json!({"weight_grams":70000,"cost_cents":1}),
        json!({"weight_grams":70000,"category":"symptom"})
    ] {
        let mut request = json!({"date":"2026-01-01","category":"weight","title":"Weight"});
        request.as_object_mut().unwrap().extend(data.as_object().unwrap().clone());
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&request).send().await.unwrap();
        assert!(res.status().is_client_error(), "{request}: {}",res.status());
    }
    let op_id = uuid::Uuid::new_v4().to_string();
    let input = json!({"date":"2026-01-01","category":"weight","title":"Weight","weight_grams":70500,"client_op_id":op_id});
    let a: Value = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&input).send().await.unwrap().json().await.unwrap();
    let replay: Value = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&input).send().await.unwrap().json().await.unwrap();
    assert_eq!(a["id"],replay["id"]);
    let mut input = json!({"date":"2026-01-01","category":"weight","title":"Corrected","weight_grams":69000});
    let url = app.url(&format!("/activities/{}",a["id"]));
    assert_eq!(app.client.patch(&url).json(&input).send().await.unwrap().status(),200);
    input.as_object_mut().unwrap().remove("weight_grams");
    let updated: Value = app.client.patch(&url).json(&input).send().await.unwrap().json().await.unwrap();
    assert_eq!(updated["weight_grams"],69000,"omitted weight stays stored");
    input["weight_grams"] = Value::Null;
    assert_eq!(app.client.patch(&url).json(&input).send().await.unwrap().status(),400);
    for (field,value,expected) in [("weight_grams",json!(68000),"accepted"),("weight_grams",Value::Null,"rejected"),("category",json!("other"),"rejected"),("cost_cents",json!(5),"rejected")] {
        let result: Value = app.client.post(app.url("/sync/push")).json(&json!({"ops":[{"entity":"activity","entity_uuid":a["client_uuid"],"op":"set","field":field,"value":value,"client_op_id":uuid::Uuid::new_v4().to_string(),"device_id":"weight-test","edited_at":"2090-01-01T00:00:00.000000Z"}]})).send().await.unwrap().json().await.unwrap();
        assert_eq!(result["results"][0]["outcome"], expected, "{result}");
    }
    let bootstrap = read(&app, "/sync/bootstrap").await;
    assert_eq!(bootstrap["activities"][0]["weight_grams"],68000);
}

#[tokio::test]
async fn body_reminders_follow_only_weight_and_export_round_trips() {
    let app = common::spawn().await; app.setup("ben", "correct horse").await;
    let id = body(&app).await;
    let response = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({"title":"Weigh in","kind":"reading","every_n":1,"every_unit":"week","due_date":"2026-01-01"})).send().await.unwrap();
    assert_eq!(response.status(),201,"{}",response.text().await.unwrap());
    let w = weight(&app,id,72000,"2026-02-01").await;
    app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({"date":"2026-03-01","category":"symptom","title":"Headache"})).send().await.unwrap();
    let reminders = read(&app,&format!("/objects/{id}/reminders")).await;
    assert_eq!(reminders[0]["last_reading_date"],"2026-02-01");
    assert_eq!(reminders[0]["next_due_date"],"2026-02-08");
    let exported = app.client.get(app.url("/export")).send().await.unwrap();
    assert_eq!(exported.status(),200);
    let archive = exported.bytes().await.unwrap();
    let imported = app.client.post(app.url("/import")).header("content-type","application/zip").body(archive).send().await.unwrap();
    assert_eq!(imported.status(),200,"{}",imported.text().await.unwrap());
    let list = read(&app,"/objects").await;
    assert_eq!(list.as_array().unwrap().len(),2);
    for object in list.as_array().unwrap() { assert_eq!(object["weight_unit"],"lb"); assert_eq!(object["stats"]["latest_weight_grams"],72000); }
    app.client.delete(app.url(&format!("/activities/{}",w["id"]))).send().await.unwrap();
    assert!(read(&app,&format!("/objects/{id}/reminders")).await[0]["last_reading_date"].is_null());
}

#[tokio::test]
async fn weight_history_is_private_and_unit_changes_preserve_grams() {
    let app = common::spawn().await; app.setup("ben", "correct horse").await;
    let id = body(&app).await;
    weight(&app,id,72350,"2026-01-01").await;
    let other = app.create_user_client("anna", "password123").await;
    assert_eq!(other.get(app.url(&format!("/objects/{id}/weight"))).send().await.unwrap().status(),404);
    for unit in [Some("kg"),None,Some("lb")] {
        let mut input = json!({"name":"Body","type":"body"});
        if let Some(unit) = unit { input["weight_unit"] = json!(unit); }
        let response = app.client.patch(app.url(&format!("/objects/{id}"))).json(&input).send().await.unwrap();
        assert_eq!(response.status(),200);
        let updated: Value = response.json().await.unwrap();
        assert_eq!(updated["stats"]["latest_weight_grams"],72350);
        assert_eq!(updated["weight_unit"],unit.unwrap_or("kg"));
    }
    let response = app.client.patch(app.url(&format!("/objects/{id}"))).json(&json!({"name":"Body","type":"body","weight_unit":"stone"})).send().await.unwrap();
    assert_eq!(response.status(),400);
    let response = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({"title":"Bad counter","kind":"service","due_counter":10})).send().await.unwrap();
    assert_eq!(response.status(),400,"Body weight must not act as a counter for maintenance");
}
