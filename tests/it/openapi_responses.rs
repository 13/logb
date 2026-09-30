//! Keeps `docs/openapi.json`'s response schemas honest, the way `openapi.rs` keeps its route list
//! honest.
//!
//! One scenario drives every documented operation that answers JSON -- a real server, real
//! data -- and checks each answer against the schema the document gives for the status it came
//! back with. Every mismatch is collected before anything fails, so one run lists everything the
//! document gets wrong rather than the first thing.
//!
//! The validator is a small one written here rather than a JSON Schema crate, for two reasons.
//! The document uses a dozen keywords, and a validator that refuses any keyword it does not
//! implement cannot silently skip a constraint. And it can be stricter than JSON Schema where
//! that is the point: an object schema that lists `properties` is read as *closed*, so a field
//! the server sends and the document does not mention fails here -- which is exactly the drift a
//! plain validator would wave through, since JSON Schema leaves objects open by default.

use crate::common::{self, TestApp};
use reqwest::{Client, Method};
use serde_json::{json, Value};
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------------------------
// The validator
// ---------------------------------------------------------------------------------------------

/// Keywords that only describe, and never constrain, a value.
const ANNOTATIONS: &[&str] =
    &["description", "default", "examples", "example", "writeOnly", "readOnly", "title", "deprecated"];

struct Spec {
    doc: Value,
}

impl Spec {
    fn load() -> Spec {
        let raw = std::fs::read_to_string("docs/openapi.json").expect("docs/openapi.json should exist");
        Spec { doc: serde_json::from_str(&raw).expect("docs/openapi.json should be valid JSON") }
    }

    /// `#/components/schemas/Name` and the like, followed until it is not a `$ref` any more.
    fn resolve<'a>(&'a self, mut node: &'a Value) -> &'a Value {
        for _ in 0..16 {
            let Some(r) = node.get("$ref").and_then(Value::as_str) else { return node };
            let pointer = r.strip_prefix('#').unwrap_or_else(|| panic!("only local $refs are supported: {r}"));
            node = self.doc.pointer(pointer).unwrap_or_else(|| panic!("$ref {r} names nothing"));
        }
        panic!("a $ref chain longer than 16 links")
    }

    /// The property names a schema allows on an object, `allOf` branches and `$ref`s included --
    /// for deciding what counts as undocumented when the schema is composed.
    fn property_names(&self, schema: &Value, into: &mut BTreeSet<String>) {
        let schema = self.resolve(schema);
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            into.extend(props.keys().cloned());
        }
        for branch in schema.get("allOf").and_then(Value::as_array).into_iter().flatten() {
            self.property_names(branch, into);
        }
    }

    fn validate(&self, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
        self.check(schema, value, at, errors, true);
    }

    /// `closed`: whether an object schema listing `properties` refuses others. Off only for an
    /// `allOf` branch, whose siblings may document the rest.
    fn check(&self, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>, closed: bool) {
        let Some(map) = schema.as_object() else {
            if schema == &Value::Bool(false) {
                errors.push(format!("{at}: no value is allowed here"));
            }
            return;
        };
        for (keyword, arg) in map {
            match keyword.as_str() {
                "$ref" => self.check(self.resolve(schema), value, at, errors, closed),
                "type" => {
                    let allowed: Vec<&str> = match arg {
                        Value::String(t) => vec![t.as_str()],
                        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
                        _ => panic!("{at}: `type` must be a string or an array"),
                    };
                    if !allowed.iter().any(|t| is_type(value, t)) {
                        errors.push(format!("{at}: expected {allowed:?}, got {}", short(value)));
                    }
                }
                "enum" => {
                    if !arg.as_array().unwrap().contains(value) {
                        errors.push(format!("{at}: {} is not one of {arg}", short(value)));
                    }
                }
                "const" => {
                    if arg != value {
                        errors.push(format!("{at}: {} is not {arg}", short(value)));
                    }
                }
                "format" => check_format(arg.as_str().unwrap(), value, at, errors),
                "minimum" | "maximum" => {
                    if let (Some(v), Some(bound)) = (value.as_f64(), arg.as_f64()) {
                        if (keyword == "minimum" && v < bound) || (keyword == "maximum" && v > bound) {
                            errors.push(format!("{at}: {v} is outside {keyword} {bound}"));
                        }
                    }
                }
                "minLength" | "maxLength" => {
                    if let Some(s) = value.as_str() {
                        let n = s.chars().count() as u64;
                        let bound = arg.as_u64().unwrap();
                        if (keyword == "minLength" && n < bound) || (keyword == "maxLength" && n > bound) {
                            errors.push(format!("{at}: length {n} is outside {keyword} {bound}"));
                        }
                    }
                }
                "minItems" | "maxItems" => {
                    if let Some(items) = value.as_array() {
                        let n = items.len() as u64;
                        let bound = arg.as_u64().unwrap();
                        if (keyword == "minItems" && n < bound) || (keyword == "maxItems" && n > bound) {
                            errors.push(format!("{at}: {n} items is outside {keyword} {bound}"));
                        }
                    }
                }
                "items" => {
                    for (i, item) in value.as_array().into_iter().flatten().enumerate() {
                        self.check(arg, item, &format!("{at}[{i}]"), errors, true);
                    }
                }
                "required" => {
                    if let Some(obj) = value.as_object() {
                        for name in arg.as_array().unwrap().iter().filter_map(Value::as_str) {
                            if !obj.contains_key(name) {
                                errors.push(format!("{at}: required property `{name}` is missing"));
                            }
                        }
                    }
                }
                "properties" => {
                    let Some(obj) = value.as_object() else { continue };
                    let props = arg.as_object().unwrap();
                    for (name, v) in obj {
                        let here = format!("{at}.{name}");
                        match props.get(name) {
                            Some(s) => self.check(s, v, &here, errors, true),
                            None => match map.get("additionalProperties") {
                                Some(extra) => self.check(extra, v, &here, errors, true),
                                None if closed => errors.push(format!(
                                    "{here}: the server sends this property and the document does not describe it ({})",
                                    short(v)
                                )),
                                None => {}
                            },
                        }
                    }
                }
                "additionalProperties" => {
                    // With `properties` beside it, handled there; alone, it covers every key.
                    if !map.contains_key("properties") {
                        for (name, v) in value.as_object().into_iter().flatten() {
                            self.check(arg, v, &format!("{at}.{name}"), errors, true);
                        }
                    }
                }
                "allOf" => {
                    let branches = arg.as_array().unwrap();
                    for b in branches {
                        self.check(b, value, at, errors, false);
                    }
                    if let (true, Some(obj)) = (closed, value.as_object()) {
                        let mut known = BTreeSet::new();
                        for b in branches {
                            self.property_names(b, &mut known);
                        }
                        for name in obj.keys().filter(|k| !known.contains(*k)) {
                            errors.push(format!(
                                "{at}.{name}: the server sends this property and the document does not describe it"
                            ));
                        }
                    }
                }
                "oneOf" | "anyOf" => {
                    let passing = arg
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|b| {
                            let mut e = Vec::new();
                            self.check(b, value, at, &mut e, closed);
                            e.is_empty()
                        })
                        .count();
                    let ok = if keyword == "oneOf" { passing == 1 } else { passing >= 1 };
                    if !ok {
                        errors.push(format!("{at}: {passing} of the {keyword} branches match {}", short(value)));
                    }
                }
                k if ANNOTATIONS.contains(&k) => {}
                other => panic!(
                    "{at}: the document uses `{other}`, which this validator does not implement -- \
                     implement it here rather than letting it go unchecked"
                ),
            }
        }
    }
}

fn is_type(value: &Value, t: &str) -> bool {
    match t {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64() || value.as_f64().is_some_and(|f| f.fract() == 0.0),
        other => panic!("unknown JSON Schema type `{other}`"),
    }
}

/// The formats the document uses, checked where a wrong one would mislead a client.
fn check_format(format: &str, value: &Value, at: &str, errors: &mut Vec<String>) {
    let Some(s) = value.as_str() else {
        if format == "int64" && value.is_number() && !(value.is_i64() || value.is_u64()) {
            errors.push(format!("{at}: {value} is not an int64"));
        }
        return;
    };
    let ok = match format {
        "date" => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() && s.len() == 10,
        "date-time" => chrono::DateTime::parse_from_rfc3339(s).is_ok(),
        "month" => chrono::NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d").is_ok() && s.len() == 7,
        "uri" => s.split_once(':').is_some_and(|(scheme, _)| !scheme.is_empty()),
        "int64" | "binary" => true,
        other => panic!("{at}: format `{other}` is not known to this validator"),
    };
    if !ok {
        errors.push(format!("{at}: {s:?} is not a {format}"));
    }
}

fn short(value: &Value) -> String {
    let s = value.to_string();
    if s.len() > 120 { format!("{}...", &s[..s.floor_char_boundary(117)]) } else { s }
}

// ---------------------------------------------------------------------------------------------
// Driving the server
// ---------------------------------------------------------------------------------------------

/// Sends requests, checks each answer against the document, and remembers what it covered.
struct Checker {
    spec: Spec,
    base: String,
    errors: Vec<String>,
    covered: BTreeSet<(String, String)>,
}

enum Body {
    None,
    Json(Value),
    Raw(&'static str, Vec<u8>),
    Form(reqwest::multipart::Form),
}

impl Checker {
    /// Sends `method concrete` as `client`, checks the answer against what the document says
    /// `method template` answers with that status, and hands back the parsed body (or Null).
    async fn call(&mut self, client: &Client, method: Method, template: &str, concrete: &str, body: Body) -> Value {
        // A path with its own `servers` entry of `/` (only `/metrics`) is served at the root
        // rather than under `/api`.
        let base = match self.spec.doc["paths"][template]["servers"][0]["url"].as_str() {
            Some("/") => self.base.strip_suffix("/api").unwrap(),
            _ => self.base.as_str(),
        };
        let mut req = client.request(method.clone(), format!("{base}{concrete}"));
        req = match body {
            Body::None => req,
            Body::Json(v) => req.json(&v),
            Body::Raw(ct, bytes) => req.header("content-type", ct).body(bytes),
            Body::Form(f) => req.multipart(f),
        };
        let res = req.send().await.unwrap();
        let status = res.status().as_u16();
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = res.bytes().await.unwrap();
        let m = method.as_str().to_ascii_lowercase();
        self.covered.insert((m.clone(), template.to_string()));
        let what = format!("{} {concrete} -> {status}", method.as_str());

        let Some(op) = self.spec.doc["paths"][template].get(&m) else {
            panic!("{method} {template} is not in docs/openapi.json");
        };
        let Some(response) = op["responses"].get(status.to_string()) else {
            self.errors.push(format!(
                "{what}: status {status} is not documented (documented: {:?}); body {}",
                op["responses"].as_object().unwrap().keys().collect::<Vec<_>>(),
                String::from_utf8_lossy(&bytes[..bytes.len().min(300)])
            ));
            return Value::Null;
        };
        let response = self.spec.resolve(response).clone();
        let is_json = content_type.starts_with("application/json");
        let parsed: Value = if is_json { serde_json::from_slice(&bytes).unwrap() } else { Value::Null };
        match response.get("content") {
            Some(content) => {
                if let Some(media) = content.get("application/json") {
                    if !is_json {
                        self.errors.push(format!("{what}: documented as JSON, answered `{content_type}`"));
                    } else if let Some(schema) = media.get("schema") {
                        let mut errs = Vec::new();
                        self.spec.validate(schema, &parsed, "$", &mut errs);
                        self.errors.extend(errs.into_iter().map(|e| format!("{what}: {e}")));
                    }
                } else if is_json {
                    self.errors.push(format!("{what}: answered JSON, documented as {:?}", content.as_object().unwrap().keys()));
                }
            }
            None if is_json || !bytes.is_empty() => self.errors.push(format!(
                "{what}: documented with no body, answered `{content_type}`: {}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(300)])
            )),
            None => {}
        }
        parsed
    }

    async fn get(&mut self, client: &Client, template: &str, concrete: &str) -> Value {
        self.call(client, Method::GET, template, concrete, Body::None).await
    }

    async fn add_activity(&mut self, client: &Client, path: &str, activity: Value) -> Value {
        self.call(client, Method::POST, "/objects/{id}/activities", path, Body::Json(activity)).await
    }
}

/// A minimal stand-in for the Telegram Bot API: enough for a token to be saved and a test
/// message to be sent.
async fn telegram_stub() -> String {
    use axum::routing::post;
    let app = axum::Router::new()
        .route("/bot123:secret/getMe", post(|| async { axum::Json(json!({"ok": true, "result": {"username": "logb_test_bot"}})) }))
        .route("/bot123:secret/sendMessage", post(|| async { axum::Json(json!({"ok": true, "result": {}})) }))
        .route("/bot123:secret/getUpdates", post(|| async { axum::Json(json!({"ok": true, "result": []})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn png() -> Vec<u8> {
    let img = image::DynamicImage::new_rgb8(64, 48);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn id(v: &Value) -> i64 {
    v["id"].as_i64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// Operations that answer JSON and are deliberately not driven here, each with its reason.
/// Empty today: every documented JSON answer is exercised below.
const NOT_DRIVEN: &[(&str, &str, &str)] = &[];

#[tokio::test]
async fn every_json_answer_matches_its_documented_schema() {
    let telegram = telegram_stub().await;
    let app: TestApp = common::spawn_with(|c| c.telegram_api_url = telegram).await;
    let mut c = Checker { spec: Spec::load(), base: app.base.clone(), errors: Vec::new(), covered: BTreeSet::new() };
    let anon = common::new_client();
    let me = app.client.clone();
    use Body::{Json as J, None as N};

    // --- before anyone exists, and signing in -------------------------------------------------
    c.get(&anon, "/health", "/health").await;
    c.get(&anon, "/auth/status", "/auth/status").await;
    // Off in this app, so the documented 404; `metrics.rs` covers the other answers.
    c.get(&anon, "/metrics", "/metrics").await;
    let creds = json!({"username": "admin", "password": "correct horse battery"});
    c.call(&me, Method::POST, "/auth/setup", "/auth/setup", J(creds.clone())).await;
    c.call(&me, Method::POST, "/auth/login", "/auth/login", J(creds)).await;
    c.get(&me, "/auth/me", "/auth/me").await;

    let token = c.call(&me, Method::POST, "/auth/tokens", "/auth/tokens", J(json!({"name": "script"}))).await;
    c.get(&me, "/auth/tokens", "/auth/tokens").await;
    let t = format!("/auth/tokens/{}", id(&token));
    c.call(&me, Method::DELETE, "/auth/tokens/{id}", &t, N).await;
    let pair = c.call(&me, Method::POST, "/auth/pair", "/auth/pair", N).await;
    c.call(
        &anon,
        Method::POST,
        "/auth/pair/redeem",
        "/auth/pair/redeem",
        J(json!({"code": pair["code"], "device_name": "Phone"})),
    )
    .await;

    // --- objects of every shape the insights and stats answer differently for --------------
    let car = c
        .call(&me, Method::POST, "/objects", "/objects", J(json!({
            "name": "Golf", "type": "car", "counter_unit": "km", "fuel_unit": "l", "tags": ["daily"],
            "purchase_date": "2020-05-01", "purchase_price_cents": 1_500_000, "fuel_capacity_milli": 50_000
        })))
        .await;
    let car_id = id(&car);
    let ev = c
        .call(&me, Method::POST, "/objects", "/objects", J(json!({
            "name": "Zoe", "type": "car", "counter_unit": "km", "fuel_unit": "kwh", "energy_price_milli": 300
        })))
        .await;
    let body = c
        .call(&me, Method::POST, "/objects", "/objects", J(json!({"name": "Me", "type": "body", "weight_unit": "kg"})))
        .await;
    let meter = c
        .call(&me, Method::POST, "/objects", "/objects", J(json!({
            "name": "Water", "type": "home", "resource_kind": "water", "resource_unit": "m3",
            "measurement_mode": "meter", "monthly_target_milli": 4_000
        })))
        .await;
    c.call(&me, Method::POST, "/objects", "/objects", J(json!({"name": "Roof box", "type": "other", "parent_id": car_id})))
        .await;
    c.get(&me, "/objects", "/objects").await;
    c.get(&me, "/objects", "/objects?all=true").await;
    let o = format!("/objects/{car_id}");
    c.get(&me, "/objects/{id}", &o).await;
    c.call(&me, Method::PATCH, "/objects/{id}", &o, J(json!({
        "name": "Golf VII", "type": "car", "counter_unit": "km", "fuel_unit": "l", "tags": ["daily", "family"]
    })))
    .await;

    // --- activities ----------------------------------------------------------------------------
    let today = chrono::Utc::now().date_naive();
    let day = |back: i64| (today - chrono::Duration::days(back)).format("%Y-%m-%d").to_string();
    let acts = format!("/objects/{car_id}/activities");
    let fill1 = c.add_activity(&me, &acts, json!({
        "date": day(40), "category": "fuel", "title": "Fill", "counter_value": 10_000,
        "quantity_milli": 40_000, "cost_cents": 7_000
    }))
    .await;
    c.add_activity(&me, &acts, json!({
        "date": day(10), "category": "fuel", "title": "Fill", "counter_value": 10_600,
        "quantity_milli": 38_000, "cost_cents": 6_800, "fuel_level_pct": 20
    }))
    .await;
    c.add_activity(&me, &acts, json!({
        "date": day(5), "category": "trip", "title": "To work", "start_counter": 10_600,
        "counter_value": 10_640, "from_place": "Home", "to_place": "Office", "duration_minutes": 35
    }))
    .await;
    let service = c.add_activity(&me, &acts, json!({
        "date": day(3), "category": "maintenance", "title": "Oil change", "notes": "5W-30",
        "cost_cents": 12_000, "tags": ["service"], "client_op_id": "op-1"
    }))
    .await;
    c.add_activity(&me, &format!("/objects/{}/activities", id(&ev)), json!({
        "date": day(2), "category": "fuel", "title": "Charge", "counter_value": 2_000,
        "quantity_milli": 30_000, "cost_cents": 900, "charged_full": 1
    }))
    .await;
    c.add_activity(&me, &format!("/objects/{}/activities", id(&body)), json!({
        "date": day(1), "category": "weight", "title": "Weight", "weight_grams": 72_500
    }))
    .await;
    for (back, reading) in [(35, 100_000), (1, 102_500)] {
        c.add_activity(&me, &format!("/objects/{}/activities", id(&meter)), json!({
            "date": day(back), "category": "usage", "title": "", "meter_reading_milli": reading
        }))
        .await;
    }
    c.get(&me, "/objects/{id}/activities", &acts).await;
    c.get(&me, "/objects/{id}/activities", &format!("{acts}?category=fuel&limit=5")).await;
    let a = format!("/activities/{}", id(&service));
    c.get(&me, "/activities/{id}", &a).await;
    c.call(&me, Method::PATCH, "/activities/{id}", &a, J(json!({
        "date": day(3), "category": "maintenance", "title": "Oil and filter", "cost_cents": 13_000
    })))
    .await;

    // --- attachments and files ------------------------------------------------------------------
    let form = reqwest::multipart::Form::new()
        .part("file", reqwest::multipart::Part::bytes(png()).file_name("front.png").mime_str("image/png").unwrap())
        .text("activity_id", id(&fill1).to_string());
    let att = c
        .call(&me, Method::POST, "/objects/{id}/attachments", &format!("/objects/{car_id}/attachments"), Body::Form(form))
        .await;
    c.get(&me, "/objects/{id}/attachments", &format!("/objects/{car_id}/attachments")).await;
    let at = format!("/attachments/{}", id(&att));
    c.call(&me, Method::PATCH, "/attachments/{id}", &at, J(json!({"caption": "Receipt"}))).await;
    let file = att["file_id"].as_i64().unwrap();
    c.get(&me, "/files/{id}", &format!("/files/{file}")).await;
    c.get(&me, "/files/{id}/thumb", &format!("/files/{file}/thumb")).await;

    // --- reminders ------------------------------------------------------------------------------
    let rems = format!("/objects/{car_id}/reminders");
    let rem = c
        .call(&me, Method::POST, "/objects/{id}/reminders", &rems, J(json!({
            "title": "Inspection", "due_date": "2020-01-01", "repeat_months": 12
        })))
        .await;
    c.call(&me, Method::POST, "/objects/{id}/reminders", &rems, J(json!({"title": "Tyres", "due_counter": 15_000})))
        .await;
    c.get(&me, "/objects/{id}/reminders", &rems).await;
    c.get(&me, "/reminders/due", "/reminders/due?within_days=30").await;
    let r = format!("/reminders/{}", id(&rem));
    c.get(&me, "/reminders/{id}", &r).await;
    c.call(&me, Method::PATCH, "/reminders/{id}", &r, J(json!({
        "title": "Inspection", "notes": "TÜV", "due_date": "2020-01-01", "repeat_months": 24
    })))
    .await;
    c.call(&me, Method::POST, "/reminders/{id}/snooze", &format!("{r}/snooze"), J(json!({"days": 7}))).await;
    c.call(&me, Method::DELETE, "/reminders/{id}/snooze", &format!("{r}/snooze"), N).await;
    c.call(&me, Method::POST, "/reminders/{id}/done", &format!("{r}/done"), J(json!({"activity_id": id(&service)})))
        .await;

    // --- per-object reads -----------------------------------------------------------------------
    c.get(&me, "/objects/{id}/insights", &format!("{o}/insights")).await;
    c.get(&me, "/objects/{id}/insights", &format!("{o}/insights?contents=true")).await;
    c.get(&me, "/objects/{id}/usage", &format!("{o}/usage")).await;
    c.get(&me, "/objects/{id}/recent-titles", &format!("{o}/recent-titles")).await;
    c.get(&me, "/objects/{id}/last-done", &format!("{o}/last-done")).await;
    c.get(&me, "/objects/{id}/trip-places", &format!("{o}/trip-places")).await;
    c.get(&me, "/objects/{id}/trips/summary", &format!("{o}/trips/summary")).await;
    c.get(&me, "/objects/{id}/energy", &format!("/objects/{}/energy", id(&ev))).await;
    c.get(&me, "/objects/{id}/energy", &format!("{o}/energy")).await;
    c.get(&me, "/objects/{id}/weight", &format!("/objects/{}/weight", id(&body))).await;
    c.get(&me, "/objects/{id}/weight/summary", &format!("/objects/{}/weight/summary", id(&body))).await;

    // --- search, tags, types, statistics ----------------------------------------------------
    c.get(&me, "/search", "/search?q=golf").await;
    c.get(&me, "/search", "/search?q=oil").await;
    c.get(&me, "/tags", "/tags").await;
    c.get(&me, "/types", "/types").await;
    let icon = logb::domain::custom_type::CUSTOM_TYPE_ICONS[0];
    let ty = c
        .call(&me, Method::POST, "/types", "/types", J(json!({"name": "Boat", "icon": icon, "categories": ["maintenance", "repair"]})))
        .await;
    let ty_path = format!("/types/{}", id(&ty));
    c.call(&me, Method::PATCH, "/types/{id}", &ty_path, J(json!({
        "name": "Sailboat", "icon": icon, "categories": ["maintenance"], "counter_unit": "h"
    })))
    .await;
    c.get(&me, "/types", "/types").await;
    c.call(&me, Method::DELETE, "/types/{id}", &ty_path, N).await;
    c.get(&me, "/stats", "/stats").await;
    c.get(&me, "/stats", &format!("/stats?year={}&purchases=true", today.format("%Y"))).await;
    c.get(&me, "/stats/energy", "/stats/energy").await;
    c.get(&me, "/stats/fuel", "/stats/fuel?months=12").await;
    c.get(&me, "/stats/water", "/stats/water").await;

    // --- settings and notifications -----------------------------------------------------------
    c.get(&me, "/settings", "/settings").await;
    c.call(&me, Method::PUT, "/settings", "/settings", J(json!({"currency": "EUR"}))).await;
    c.get(&me, "/me/appearance", "/me/appearance").await;
    c.call(&me, Method::PUT, "/me/appearance", "/me/appearance", J(json!({
        "locale": "de", "theme": "dark", "dateFormat": "iso", "firstDayOfWeek": "monday"
    })))
    .await;
    c.get(&me, "/me/appearance", "/me/appearance").await;
    c.get(&me, "/me/notifications", "/me/notifications").await;
    c.call(&me, Method::PUT, "/me/notifications", "/me/notifications", J(json!({"url": null, "format": "text"}))).await;
    c.call(&me, Method::PUT, "/me/notifications/hour", "/me/notifications/hour", J(json!({"hour": 7, "timezone": "Europe/Berlin"})))
        .await;
    c.call(&me, Method::PUT, "/me/notifications/telegram", "/me/notifications/telegram", J(json!({"token": "123:secret"})))
        .await;
    c.call(&me, Method::POST, "/me/notifications/telegram/link", "/me/notifications/telegram/link", N).await;
    c.call(&me, Method::POST, "/me/notifications/test", "/me/notifications/test", N).await;
    c.call(&me, Method::POST, "/me/notifications/telegram/unlink", "/me/notifications/telegram/unlink", N).await;
    c.call(&me, Method::DELETE, "/me/notifications/telegram", "/me/notifications/telegram", N).await;
    // The generator point of P-256 is a valid public key, which is all a subscription is checked
    // for; nothing is ever sent to it.
    let endpoint = "https://push.example.invalid/sub/1";
    c.call(&me, Method::POST, "/push/subscriptions", "/push/subscriptions", J(json!({
        "endpoint": endpoint,
        "keys": {
            "p256dh": "BGsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKWT-NC4v4af5uO5-tKfA-eFivOM1drMV7Oy7ZAaDe_UfU",
            "auth": "AwMDAwMDAwMDAwMDAwMDAw"
        }
    })))
    .await;
    c.get(&me, "/me/notifications", "/me/notifications").await;
    c.call(&me, Method::DELETE, "/push/subscriptions", "/push/subscriptions", J(json!({"endpoint": endpoint}))).await;

    // --- users ---------------------------------------------------------------------------------
    let user = c
        .call(&me, Method::POST, "/users", "/users", J(json!({"username": "anna", "password": "password123"})))
        .await;
    c.get(&me, "/users", "/users").await;
    let u = format!("/users/{}", id(&user));
    c.call(&me, Method::PATCH, "/users/{id}", &u, J(json!({"password": "password456"}))).await;
    c.call(&me, Method::DELETE, "/users/{id}", &u, N).await;

    // --- sync -----------------------------------------------------------------------------------
    c.get(&me, "/sync/bootstrap", "/sync/bootstrap").await;
    let epoch = logb::sync::epoch::current(&app.state.db).await.unwrap();
    c.get(&me, "/sync/pull", &format!("/sync/pull?since=0&epoch={epoch}&limit=500")).await;
    c.call(&me, Method::POST, "/sync/push", "/sync/push", J(json!({"ops": [
        {
            "client_op_id": "sync-1", "entity": "object", "entity_uuid": car["client_uuid"], "op": "set",
            "field": "name", "value": "Golf VIII", "edited_at": "2090-01-01T00:00:00.000Z", "device_id": "phone"
        },
        {
            "client_op_id": "sync-2", "entity": "object", "entity_uuid": car["client_uuid"], "op": "set",
            "field": "no_such_column", "value": 1, "edited_at": "2090-01-01T00:00:00.000Z", "device_id": "phone"
        }
    ]})))
    .await;
    c.get(&me, "/sync/pull", &format!("/sync/pull?since=0&epoch={epoch}")).await;

    // --- export and import ----------------------------------------------------------------------
    let zip = me.get(app.url("/export")).send().await.unwrap().bytes().await.unwrap().to_vec();
    c.get(&me, "/export", "/export").await;
    c.call(&me, Method::POST, "/import", "/import", Body::Raw("application/zip", zip)).await;

    // --- deletes --------------------------------------------------------------------------------
    c.call(&me, Method::DELETE, "/attachments/{id}", &at, N).await;
    c.call(&me, Method::DELETE, "/activities/{id}", &a, N).await;
    c.call(&me, Method::DELETE, "/reminders/{id}", &r, N).await;
    c.call(&me, Method::DELETE, "/objects/{id}", &format!("/objects/{}", id(&meter)), N).await;

    // --- the database screen, last: a switch writes a pointer and a restart asks to stop ------
    c.get(&me, "/database", "/database").await;
    c.get(&me, "/database/backup", "/database/backup").await;
    let scratch = tempfile::tempdir().unwrap();
    let dest = logb::db::sqlite_url(scratch.path()).unwrap();
    c.call(&me, Method::POST, "/database/test", "/database/test", J(json!({"url": dest}))).await;
    // Answers 200 on SQLite; on PostgreSQL the harness sets LOGB_DATABASE_URL, and the refusal
    // that earns is a documented 400 checked the same way.
    c.call(&me, Method::POST, "/database/switch", "/database/switch", J(json!({"url": dest}))).await;
    c.call(&me, Method::POST, "/database/restart", "/database/restart", N).await;
    c.call(&me, Method::POST, "/auth/logout-all", "/auth/logout-all", N).await;
    c.call(&me, Method::POST, "/auth/logout", "/auth/logout", N).await;

    // --- everything documented as answering JSON was driven -----------------------------------
    let mut undriven = Vec::new();
    for (path, ops) in c.spec.doc["paths"].as_object().unwrap() {
        for (method, op) in ops.as_object().unwrap() {
            let answers_json = op["responses"]
                .as_object()
                .into_iter()
                .flatten()
                .any(|(_, r)| c.spec.resolve(r).pointer("/content/application~1json").is_some());
            let key = (method.clone(), path.clone());
            if answers_json
                && !c.covered.contains(&key)
                && !NOT_DRIVEN.iter().any(|(m, p, _)| *m == method && *p == path)
            {
                undriven.push(format!("{method} {path}"));
            }
        }
    }
    assert!(undriven.is_empty(), "documented JSON operations this test does not drive: {undriven:#?}");
    assert!(c.errors.is_empty(), "docs/openapi.json disagrees with the server:\n{}", c.errors.join("\n"));
}

/// The validator itself: a schema that should refuse must refuse, or the test above proves
/// nothing.
#[test]
fn the_validator_refuses_what_it_should() {
    let spec = Spec {
        doc: json!({"components": {"schemas": {
            "Thing": {"type": "object", "required": ["id"], "properties": {
                "id": {"type": "integer", "format": "int64"},
                "at": {"type": ["string", "null"], "format": "date-time"},
                "kind": {"type": "string", "enum": ["a", "b"]}
            }},
            "More": {"allOf": [{"$ref": "#/components/schemas/Thing"}, {"type": "object", "properties": {"extra": {"type": "string"}}}]}
        }}}),
    };
    let thing = json!({"$ref": "#/components/schemas/Thing"});
    let more = json!({"$ref": "#/components/schemas/More"});
    let errors = |schema: &Value, value: Value| {
        let mut e = Vec::new();
        spec.validate(schema, &value, "$", &mut e);
        e
    };
    assert!(errors(&thing, json!({"id": 1, "at": null, "kind": "a"})).is_empty());
    assert!(errors(&thing, json!({"id": 1, "at": "2026-01-01T00:00:00Z"})).is_empty());
    assert_eq!(errors(&thing, json!({"at": null})).len(), 1, "missing required");
    assert_eq!(errors(&thing, json!({"id": "1"})).len(), 1, "wrong type");
    assert_eq!(errors(&thing, json!({"id": 1, "kind": "c"})).len(), 1, "not in enum");
    assert_eq!(errors(&thing, json!({"id": 1, "at": "2026-01-01 00:00"})).len(), 1, "bad date-time");
    assert_eq!(errors(&thing, json!({"id": 1, "surprise": true})).len(), 1, "undocumented property");
    assert!(errors(&more, json!({"id": 1, "extra": "x"})).is_empty(), "allOf sees both branches' properties");
    assert_eq!(errors(&more, json!({"id": 1, "surprise": 1})).len(), 1, "allOf is still closed");
}
