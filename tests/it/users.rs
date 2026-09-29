use crate::common;
use serde_json::json;

#[tokio::test]
async fn admin_manages_users() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/users")).json(&json!({ "username": "anna", "password": "password123" })).send().await.unwrap();
    assert_eq!(res.status(), 201);
    let anna: serde_json::Value = res.json().await.unwrap();
    assert_eq!(anna["is_admin"], false);

    let list: Vec<serde_json::Value> = app.client.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 2);

    let res = app.client.post(app.url("/users")).json(&json!({ "username": "ANNA", "password": "password123" })).send().await.unwrap();
    assert_eq!(res.status(), 409, "duplicate username, case-insensitive");

    let res = app.client.patch(app.url(&format!("/users/{}", anna["id"]))).json(&json!({ "is_admin": true, "lang": "de" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let anna: serde_json::Value = res.json().await.unwrap();
    assert_eq!(anna["is_admin"], true);
    assert_eq!(anna["lang"], "de");

    let res = app.client.delete(app.url(&format!("/users/{}", anna["id"]))).send().await.unwrap();
    assert_eq!(res.status(), 204);
    let res = app.client.delete(app.url("/users/1")).send().await.unwrap();
    assert_eq!(res.status(), 400, "cannot delete yourself");
}

#[tokio::test]
async fn non_admin_is_limited_to_self() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    assert_eq!(anna.get(app.url("/users")).send().await.unwrap().status(), 403);
    assert_eq!(anna.post(app.url("/users")).json(&json!({ "username": "x", "password": "password123" })).send().await.unwrap().status(), 403);
    assert_eq!(anna.patch(app.url("/users/1")).json(&json!({ "lang": "de" })).send().await.unwrap().status(), 403);

    let me: serde_json::Value = anna.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();
    let res = anna.patch(app.url(&format!("/users/{}", me["id"]))).json(&json!({ "is_admin": true })).send().await.unwrap();
    assert_eq!(res.status(), 403, "cannot promote self");
    let res = anna.patch(app.url(&format!("/users/{}", me["id"]))).json(&json!({ "lang": "de", "password": "newpassword1", "current_password": "password123" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let c = common::new_client();
    assert_eq!(app.login(&c, "anna", "newpassword1").await.status(), 200);
}

#[tokio::test]
async fn rejected_field_does_not_leave_partial_write() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let me: serde_json::Value = app.client.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();

    // Password is valid and would apply first; is_admin is a self-demotion and must be
    // rejected. The password write must not survive that rejection.
    let res = app
        .client
        .patch(app.url(&format!("/users/{}", me["id"])))
        .json(&json!({ "password": "newpass1", "current_password": "correct horse", "is_admin": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);

    let c = common::new_client();
    assert_eq!(app.login(&c, "ben", "correct horse").await.status(), 200, "original password must still work");
    let c2 = common::new_client();
    assert_eq!(app.login(&c2, "ben", "newpass1").await.status(), 401, "rejected update must not have changed the password");
}

#[tokio::test]
async fn settings_currency() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let s: serde_json::Value = anna.get(app.url("/settings")).send().await.unwrap().json().await.unwrap();
    assert_eq!(s["currency"], "EUR");
    assert_eq!(anna.put(app.url("/settings")).json(&json!({ "currency": "CHF" })).send().await.unwrap().status(), 403);
    assert_eq!(app.client.put(app.url("/settings")).json(&json!({ "currency": "chf" })).send().await.unwrap().status(), 400);
    let s: serde_json::Value = app.client.put(app.url("/settings")).json(&json!({ "currency": "CHF" })).send().await.unwrap().json().await.unwrap();
    assert_eq!(s["currency"], "CHF");
}

/// Regression: `DELETE FROM users` alone hits `attachments.file_id ON DELETE RESTRICT` on the
/// way through the cascade, so deleting anyone who had ever uploaded a file returned a 500.
#[tokio::test]
async fn deleting_a_user_takes_their_objects_files_and_blobs() {
    use reqwest::multipart::{Form, Part};
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let eve = app.create_user_client("eve", "password123").await;

    let obj = app.create_object(&eve, "Bike", None).await;
    let oid = obj["id"].as_i64().unwrap();
    let png = {
        let img = image::DynamicImage::new_rgb8(20, 20);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let att: serde_json::Value = eve.post(app.url(&format!("/objects/{oid}/attachments")))
        .multipart(Form::new().part("file", Part::bytes(png).file_name("p.png").mime_str("image/png").unwrap()))
        .send().await.unwrap().json().await.unwrap();
    let (sha,): (String,) = sqlx::query_as("SELECT sha256 FROM files WHERE id = $1")
        .bind(att["file_id"].as_i64().unwrap()).fetch_one(&app.state.db).await.unwrap();
    assert!(app.state.storage.thumb_path(&sha).exists(), "the precondition: a thumbnail was written");

    let users: serde_json::Value = app.client.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    let eve_id = users.as_array().unwrap().iter().find(|u| u["username"] == "eve").unwrap()["id"].as_i64().unwrap();

    let res = app.client.delete(app.url(&format!("/users/{eve_id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204, "{}", res.text().await.unwrap());

    // The account is gone, and so is everything hanging off it.
    let users: serde_json::Value = app.client.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    assert_eq!(users.as_array().unwrap().len(), 1, "{users}");
    for (table, n) in [("objects", 0), ("activities", 0), ("attachments", 0), ("files", 0), ("sessions", 1)] {
        let (count,): (i64,) = match table {
            "objects" => sqlx::query_as("SELECT COUNT(*) FROM objects"),
            "activities" => sqlx::query_as("SELECT COUNT(*) FROM activities"),
            "attachments" => sqlx::query_as("SELECT COUNT(*) FROM attachments"),
            "files" => sqlx::query_as("SELECT COUNT(*) FROM files"),
            _ => sqlx::query_as("SELECT COUNT(*) FROM sessions"),
        }.fetch_one(&app.state.db).await.unwrap();
        assert_eq!(count, n, "{table} rows left");
    }
    // The blob and its thumbnail left the disk with the rows.
    assert!(!app.state.storage.thumb_path(&sha).exists(), "thumbnail survived the user");
    // `blob_path` shards on the first two hex characters, so its grandparent is `data/files`.
    let files_dir = app.state.storage.blob_path(&"0".repeat(64)).parent().unwrap().parent().unwrap().to_path_buf();
    let remaining: Vec<_> = walkdir(&files_dir);
    assert!(remaining.is_empty(), "blobs left behind: {remaining:?}");

    // Eve's session died with her account.
    assert_eq!(eve.get(app.url("/auth/me")).send().await.unwrap().status(), 401);
}

fn walkdir(p: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(p) {
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() { out.extend(walkdir(&path)); } else { out.push(path); }
        }
    }
    out
}

/// A password change has to end the sessions opened with the old password.
#[tokio::test]
async fn changing_a_password_invalidates_other_sessions() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let eve = app.create_user_client("eve", "password123").await;
    let eve_other_browser = common::new_client();
    assert_eq!(app.login(&eve_other_browser, "eve", "password123").await.status(), 200);
    let users: serde_json::Value = app.client.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    let eve_id = users.as_array().unwrap().iter().find(|u| u["username"] == "eve").unwrap()["id"].as_i64().unwrap();

    let res = eve.patch(app.url(&format!("/users/{eve_id}")))
        .json(&serde_json::json!({ "password": "a better password", "current_password": "password123" })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    // The browser that made the change stays signed in, on a new session.
    assert_eq!(eve.get(app.url("/auth/me")).send().await.unwrap().status(), 200);
    // The other one does not.
    assert_eq!(eve_other_browser.get(app.url("/auth/me")).send().await.unwrap().status(), 401);
    // And the old password no longer works.
    assert_eq!(app.login(&common::new_client(), "eve", "password123").await.status(), 401);
}

/// An admin resetting someone's password locks that someone out of every open session.
#[tokio::test]
async fn an_admin_reset_ends_the_users_sessions() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let eve = app.create_user_client("eve", "password123").await;
    let users: serde_json::Value = app.client.get(app.url("/users")).send().await.unwrap().json().await.unwrap();
    let eve_id = users.as_array().unwrap().iter().find(|u| u["username"] == "eve").unwrap()["id"].as_i64().unwrap();

    assert_eq!(app.client.patch(app.url(&format!("/users/{eve_id}")))
        .json(&serde_json::json!({ "password": "reset by the admin" })).send().await.unwrap().status(), 200);
    assert_eq!(eve.get(app.url("/auth/me")).send().await.unwrap().status(), 401);
    // The admin's own session is untouched.
    assert_eq!(app.client.get(app.url("/auth/me")).send().await.unwrap().status(), 200);
}

async fn my_id(app: &common::TestApp, client: &reqwest::Client) -> i64 {
    let me: serde_json::Value = client.get(app.url("/auth/me")).send().await.unwrap().json().await.unwrap();
    me["id"].as_i64().unwrap()
}

/// A session left open on a shared computer must not be enough to take the account over: the
/// person at the keyboard has to know the password they are replacing. Missing and wrong are
/// refused alike, with a code a client can put into words, and the old password still works.
#[tokio::test]
async fn changing_your_own_password_needs_the_current_one() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let id = my_id(&app, &anna).await;

    for body in [
        json!({ "password": "new secret 1" }),
        json!({ "password": "new secret 1", "current_password": "not it at all" }),
    ] {
        let res = anna.patch(app.url(&format!("/users/{id}"))).json(&body).send().await.unwrap();
        assert_eq!(res.status(), 403, "{body}");
        let err: serde_json::Value = res.json().await.unwrap();
        assert_eq!(err["error"], "wrong_password", "{err}");
    }
    assert_eq!(app.login(&common::new_client(), "anna", "password123").await.status(), 200);
    // The session that asked is still signed in: a refusal ends nothing.
    assert_eq!(anna.get(app.url("/auth/me")).send().await.unwrap().status(), 200);

    // An admin is asked for their own current password too; being an admin is not knowing it.
    let ben = my_id(&app, &app.client).await;
    let res = app.client.patch(app.url(&format!("/users/{ben}"))).json(&json!({ "password": "new secret 1" })).send().await.unwrap();
    assert_eq!(res.status(), 403);
}

/// A change that does not touch the password needs no password.
#[tokio::test]
async fn changing_only_your_language_needs_no_password() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let id = my_id(&app, &anna).await;
    let res = anna.patch(app.url(&format!("/users/{id}"))).json(&json!({ "lang": "de" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

/// Guessing the current password through this route is guessing a password, so it spends the
/// same allowance a login does: otherwise a stolen session would be an unlimited oracle for it.
#[tokio::test]
async fn wrong_current_passwords_count_against_the_login_limit() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let id = my_id(&app, &anna).await;
    // `create_user_client` already spent one of this address's ten attempts on its login.
    for i in 0..9 {
        let res = anna.patch(app.url(&format!("/users/{id}")))
            .json(&json!({ "password": "new secret 1", "current_password": format!("guess {i}") }))
            .send().await.unwrap();
        assert_eq!(res.status(), 403, "guess {i}");
    }
    let res = anna.patch(app.url(&format!("/users/{id}")))
        .json(&json!({ "password": "new secret 1", "current_password": "password123" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 429, "the right answer is refused too once the allowance is spent");
    assert_eq!(app.login(&common::new_client(), "anna", "password123").await.status(), 429);
}
