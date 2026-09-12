//! The four endpoints Settings drives the database choice through.
//!
//! Most of this file is about one thing: a PostgreSQL connection URL carries a password, and
//! from the moment Settings can be handed one, that password is inside the running server. It
//! must not come back out -- not in a response body, and not in a log line. Every test below
//! that says so searches the actual bytes, because the only way this can be got wrong is by
//! adding a second place that formats the URL, and no test of the shape of the code notices
//! that.

mod common;

use serde_json::json;

/// The password `LOGB_TEST_DATABASE_URL` carries is `logb` on every machine this suite runs on,
/// and "logb" is also the crate name, the log target on every line this application writes, the
/// prefix of every scratch database and part of the SQLite filename. Searching for it would
/// fail on text that has leaked nothing, so the leak tests use a destination whose password
/// occurs nowhere else -- `common::SCRATCH_PASSWORD`. This is the needle for the URLs a test
/// makes up, which have no such constraint.
const INVENTED_PASSWORD: &str = "pelican-stapler-9000";

/// The offending lines, for a failure message that says where to look rather than only that
/// there is somewhere to look.
fn lines_with(text: &str, needle: &str) -> String {
    text.lines().filter(|l| l.contains(needle)).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn the_current_database_is_described_without_its_password() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let body: serde_json::Value = app.get_json("/database").await;

    assert!(body["backend"].is_string(), "{body}");
    let text = body.to_string();
    assert!(!text.contains("password"), "{text}");
    // The description is derived from the URL this instance is running on, so if anything of
    // that URL survives into it, it is these.
    let url = app.database_url();
    assert!(!text.contains(&url), "the whole connection URL is in the description: {text}");
    if let Some((_, rest)) = url.split_once("://") {
        if let Some((userinfo, _)) = rest.split_once('@') {
            assert!(!text.contains(userinfo), "the userinfo is in the description: {text}");
        }
    }
}

/// The whole point of the feature, and the whole point of the task: a URL goes in through
/// Settings, is copied onto, is remembered -- and never comes back out.
#[tokio::test]
async fn a_saved_url_never_comes_back_out() {
    let Some(server) = common::test_server_url() else {
        eprintln!("SKIPPED: needs LOGB_TEST_DATABASE_URL -- only PostgreSQL has a password to leak");
        return;
    };
    // No `database_url` in the config: the environment deciding the database is exactly the
    // case where Settings may not, so a test of switching has to be an instance that is not in
    // it. That is also the real migration -- a stock SQLite instance moving onto PostgreSQL.
    let app = common::spawn_with(|config| config.database_url = None).await;
    app.setup("ben", "correct horse").await;
    let dest = common::scratch_database_with_its_own_password(&server).await;

    app.post_json("/database/switch", &json!({ "url": dest.url })).await;

    // Neither the description nor any log line may carry it.
    let body: serde_json::Value = app.get_json("/database").await;
    assert!(
        !body.to_string().contains(dest.password),
        "the password leaked into the description: {body}"
    );
    let logs = app.captured_logs();
    assert!(!logs.contains(dest.password), "the password leaked into the logs:\n{}", lines_with(&logs, dest.password));
    assert!(!logs.contains(&dest.url), "the whole connection URL leaked into the logs:\n{}", lines_with(&logs, &dest.url));

    // ...and this is what makes the two assertions above mean something. The switch is logged,
    // and it is logged *about that destination*: the redacted form names the host it copied
    // onto. A run that logged nothing at all would pass the assertions for the wrong reason.
    let host = dest.url.rsplit_once('@').unwrap().1.split('/').next().unwrap();
    assert!(logs.contains(host), "the switch was never logged at all, so nothing was proved");

    // The pointer file is the one place the password legitimately lives, and it is 0600. If it
    // did not have the URL in it, the restart would come up on the old database.
    let pointer = std::fs::read_to_string(app.state.config.data_dir.join("database.url")).unwrap();
    assert_eq!(pointer.trim(), dest.url);
}

#[tokio::test]
async fn only_an_admin_may_switch_the_database() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let plain = app.create_user_client("anna", "password123").await;

    let res = plain
        .post(app.url("/database/switch"))
        .json(&json!({ "url": "sqlite::memory:" }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 403);
}

/// Every route in this file, not just the one above. `restart` is in the list on purpose: a
/// rejected extractor never reaches the handler, so this proves the refusal without exiting
/// the test binary -- and it is the route where getting the guard wrong is worst.
#[tokio::test]
async fn none_of_the_database_routes_answer_a_plain_user() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let plain = app.create_user_client("anna", "password123").await;

    assert_eq!(plain.get(app.url("/database")).send().await.unwrap().status(), 403);
    for path in ["/database/test", "/database/switch", "/database/restart"] {
        let res = plain.post(app.url(path)).json(&json!({ "url": "sqlite::memory:" })).send().await.unwrap();
        assert_eq!(res.status(), 403, "POST {path} answered a plain user");
    }
}

/// Testing a destination is the first thing the screen does with a URL somebody typed, which
/// makes the driver's own error text the first thing that can carry a password back out.
#[tokio::test]
async fn an_unreachable_destination_is_reported_without_its_password() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    // Port 1 refuses immediately, on every machine, without needing a PostgreSQL anywhere.
    let url = format!("postgres://someone:{INVENTED_PASSWORD}@127.0.0.1:1/nowhere");

    let body = app.post_json("/database/test", &json!({ "url": url })).await;

    assert_eq!(body["reachable"], json!(false), "{body}");
    assert_eq!(body["state"], json!("unreachable"), "{body}");
    let text = body.to_string();
    assert!(!text.contains(INVENTED_PASSWORD), "the password came back in the answer: {text}");
    assert!(!text.contains(&url), "the whole URL came back in the answer: {text}");
    assert!(
        !app.captured_logs().contains(INVENTED_PASSWORD),
        "the password of a destination that was only tested reached the logs",
    );
}

/// The ordering `switch` relies on for safety: the destination is copied onto *before* the
/// pointer is written, so a copy that fails leaves the instance exactly as it was. A switch to
/// an unreachable host must therefore both be refused and leave no pointer behind -- a pointer
/// written anyway would arm the next restart onto a database that was never populated, and the
/// refuse-to-start path would then hold the instance down until someone hand-deletes
/// `database.url`.
#[tokio::test]
async fn a_failed_switch_leaves_no_pointer_behind() {
    let app = common::spawn_with(|config| config.database_url = None).await;
    app.setup("ben", "correct horse").await;
    let url = "postgres://x:y@127.0.0.1:1/nowhere";

    let res = app.post_raw("/database/switch", &json!({ "url": url })).await;

    assert_eq!(res.status(), 400, "{}", res.text().await.unwrap());
    assert_eq!(
        logb::pointer::read(&app.state.config.data_dir),
        None,
        "a switch that failed to copy still wrote the pointer",
    );
}

#[tokio::test]
async fn a_database_with_users_in_it_is_reported_as_holding_logb_data() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    let body = app.post_json("/database/test", &json!({ "url": app.database_url() })).await;

    assert_eq!(body["reachable"], json!(true), "{body}");
    assert_eq!(body["state"], json!("holds_logb_data"), "{body}");
}

#[tokio::test]
async fn an_empty_destination_is_reported_as_empty() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let empty = common::scratch_database().await;

    let body = app.post_json("/database/test", &json!({ "url": empty.url })).await;

    assert_eq!(body["reachable"], json!(true), "{body}");
    assert_eq!(body["state"], json!("empty"), "{body}");
    // And it stayed empty: a test that migrated the destination would have written eleven
    // tables into a database the operator had only asked a question about.
    assert_eq!(empty.user_count_or_no_schema().await, None, "the probe built a schema");
}

#[tokio::test]
async fn switching_copies_the_database_and_remembers_the_destination() {
    let app = common::spawn_with(|config| config.database_url = None).await;
    app.setup("ben", "correct horse").await;
    let dest = common::scratch_database().await;

    let body = app.post_json("/database/switch", &json!({ "url": dest.url })).await;

    assert_eq!(body["restart_required"], json!(true), "{body}");
    assert!(body["epoch"].is_string(), "{body}");
    let users = body["tables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["table"] == json!("users"))
        .unwrap_or_else(|| panic!("no users table in the report: {body}"));
    assert_eq!(users["rows"], json!(1), "{body}");
    // The data is really there, in the other database.
    assert_eq!(dest.user_count().await, 1);
    // And the pointer now names it, so the next start opens it instead.
    assert_eq!(logb::pointer::read(&app.state.config.data_dir).as_deref(), Some(dest.url.as_str()));

    // The instance is still serving the database it was on -- the copy is not a handover, the
    // restart is. The description says both halves, so the screen can ask for one.
    let described = app.get_json("/database").await;
    assert!(described["pending"].is_object(), "the pending destination is not described: {described}");
}

#[tokio::test]
async fn copying_a_database_onto_itself_is_refused_for_the_reason_it_actually_is() {
    let app = common::spawn_with(|config| config.database_url = None).await;
    app.setup("ben", "correct horse").await;

    let res = app.post_raw("/database/switch", &json!({ "url": app.database_url() })).await;

    assert_eq!(res.status(), 400);
    let body: serde_json::Value = res.json().await.unwrap();
    let message = body["message"].as_str().unwrap_or_default().to_string();
    // `copy::run_live` never sees the source URL and so cannot make this comparison itself:
    // without it the answer is "the destination already holds data", which sends an operator
    // looking for a second instance that does not exist.
    assert!(
        message.contains("already"),
        "a database copied onto itself should say so: {message}",
    );
    assert!(
        !message.contains("holds data"),
        "copying onto itself was reported as a non-empty destination: {message}",
    );
}

/// An instance whose database is named by its environment cannot have that changed from a
/// browser: the pointer file is read only when `LOGB_DATABASE_URL` is unset, so writing one
/// here would be a change that silently does nothing at the next start.
#[tokio::test]
async fn the_environment_keeps_the_choice_away_from_settings() {
    let app = common::spawn_with(|config| {
        config.database_url = Some(logb::db::sqlite_url(&config.data_dir).unwrap());
    })
    .await;
    app.setup("ben", "correct horse").await;
    let dest = common::scratch_database().await;

    let described = app.get_json("/database").await;
    assert_eq!(described["pointer_writable"], json!(false), "{described}");

    let res = app.post_raw("/database/switch", &json!({ "url": dest.url })).await;
    assert_eq!(res.status(), 400);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body["message"].as_str().unwrap_or_default().contains("LOGB_DATABASE_URL"),
        "the refusal should name what is deciding instead: {body}",
    );
    assert_eq!(logb::pointer::read(&app.state.config.data_dir), None, "a pointer was written anyway");
}

/// A blank or absent URL is a form somebody submitted early, not a server error.
#[tokio::test]
async fn an_empty_url_is_refused_rather_than_attempted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;

    for path in ["/database/test", "/database/switch"] {
        let res = app.post_raw(path, &json!({ "url": "   " })).await;
        assert_eq!(res.status(), 400, "POST {path} accepted a blank URL");
    }
}

/// The restart, against the real binary, because the claim being tested is about a process
/// exiting and there is no way to make that claim in the process running the tests.
///
/// What it pins is the ordering: the 202 and its whole body reach the client *before* the
/// process goes away. Exiting from inside the handler instead drops the connection, and the
/// client sees a transport error where it should see an acknowledgement -- an admin pressing
/// "restart" would be told the request failed by the one request that worked.
#[tokio::test]
async fn a_restart_is_acknowledged_before_the_process_goes_away() {
    let dir = tempfile::tempdir().unwrap();
    let port = {
        // Taken and let go: the child needs to be told a port, so it cannot ask for 0 itself.
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        taken.local_addr().unwrap().port()
    };
    let mut child = Spawned(
        std::process::Command::new(env!("CARGO_BIN_EXE_logb"))
            .env("LOGB_DATA_DIR", dir.path())
            .env("LOGB_BIND", "127.0.0.1")
            .env("LOGB_PORT", port.to_string())
            .env("LOGB_LOG", "warn")
            // Whatever the suite is pointed at, this one is a stock SQLite instance in a
            // directory of its own: the subject is the exit, not the backend.
            .env_remove("LOGB_DATABASE_URL")
            .spawn()
            .expect("the logb binary should be built and runnable"),
    );
    let base = format!("http://127.0.0.1:{port}/api");
    let client = common::new_client();
    let mut up = false;
    for _ in 0..300 {
        if let Ok(res) = client.get(format!("{base}/health")).send().await {
            if res.status().is_success() {
                up = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(up, "the spawned instance never came up");
    let res = client
        .post(format!("{base}/auth/setup"))
        .json(&json!({ "username": "ben", "password": "correct horse" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);

    let res = client.post(format!("{base}/database/restart")).send().await.unwrap();

    assert_eq!(res.status(), 202);
    // Reading the body is half the assertion: it proves the response was written whole, not
    // that a status line got out before the socket closed.
    let body: serde_json::Value = res.json().await.expect("the body should arrive complete");
    assert_eq!(body["restarting"], json!(true), "{body}");

    for _ in 0..100 {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "the process exited badly: {status}");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the process answered the restart and then stayed up");
}

/// Kills the spawned instance however this test ends, so a failing assertion does not leave a
/// server holding a port.
struct Spawned(std::process::Child);

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
