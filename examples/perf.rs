//! Performance benchmark: seeds a realistic instance and times the hot endpoints over HTTP.
//!
//! Numbers only mean something from a release build:
//!
//! ```text
//! cargo run --release --example perf                                  # SQLite in a temp dir
//! cargo run --release --example perf -- --postgres postgres://postgres:logb@127.0.0.1:55440/postgres
//! cargo run --release --example perf -- --json before.json
//! cargo run --release --example perf -- --compare before.json after.json
//! ```
//!
//! The app is built in-process through `logb::build_with_state`, exactly as the integration
//! tests do (`tests/it/common/mod.rs`), and served on a loopback port. Everything is seeded
//! through the public API, so the change log, `field_clock`, thumbnails and every other side
//! table look the way real use leaves them. The seed is fixed: two runs at the same size seed
//! the same data (ids and timestamps aside).
//!
//! `--postgres` takes a *server* URL, as `LOGB_TEST_DATABASE_URL` does: a scratch database
//! named `logb_perf_<pid>` is created on it and dropped at the end.

use clap::Parser;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(name = "perf", about = "Seed a LogB instance and time its hot endpoints")]
struct Args {
    /// PostgreSQL *server* URL to benchmark on (a scratch database is created and dropped).
    /// Without it, a SQLite file in a temporary directory is used.
    #[arg(long, env = "LOGB_PERF_DATABASE_URL")]
    postgres: Option<String>,
    /// Number of objects to seed.
    #[arg(long, default_value_t = 500)]
    objects: usize,
    /// Number of activities to seed, spread over the objects.
    #[arg(long, default_value_t = 20_000)]
    activities: usize,
    /// Number of attachments (small JPEGs, so thumbnails exist) to seed.
    #[arg(long, default_value_t = 2_000)]
    attachments: usize,
    /// Timed runs per endpoint.
    #[arg(long, default_value_t = 20)]
    runs: usize,
    /// Timed runs for export and import, which are much slower.
    #[arg(long, default_value_t = 3)]
    heavy_runs: usize,
    /// Untimed warm-up requests per endpoint.
    #[arg(long, default_value_t = 3)]
    warmup: usize,
    /// Concurrent clients while seeding.
    #[arg(long, default_value_t = 8)]
    seed_concurrency: usize,
    /// Skip export and import.
    #[arg(long)]
    skip_heavy: bool,
    /// Write the results as JSON to this file.
    #[arg(long)]
    json: Option<std::path::PathBuf>,
    /// Compare two JSON result files (before, after) and exit; nothing is seeded.
    #[arg(long, num_args = 2, value_names = ["BEFORE", "AFTER"])]
    compare: Option<Vec<std::path::PathBuf>>,
}

/// What a browser sends. The server's compression is part of what a user waits for, so the
/// timed requests ask for it too; the bodies are never decoded, only counted.
const ACCEPT_ENCODING: &str = "gzip, deflate, br, zstd";
const PASSWORD: &str = "perf-bench-password";
/// A term found in many titles and notes, and one found in exactly three notes.
const COMMON_TERM: &str = "oil";
const RARE_TERM: &str = "zebrafish";

#[derive(Serialize, Deserialize, Clone)]
struct Measurement {
    name: String,
    runs: usize,
    median_ms: f64,
    p95_ms: f64,
    min_ms: f64,
    max_ms: f64,
    /// Response body size on the wire (compressed when the server compressed it).
    bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct Report {
    backend: String,
    git: String,
    date: String,
    objects: usize,
    activities: usize,
    attachments: usize,
    seed_seconds: f64,
    results: Vec<Measurement>,
}

// ---------------------------------------------------------------------------------------------
// Deterministic randomness: splitmix64, so the dataset does not depend on a crate's version.

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

// ---------------------------------------------------------------------------------------------
// The instance.

struct Instance {
    base: String,
    _dir: tempfile::TempDir,
    /// `(server_url, database_name)` of the PostgreSQL scratch database, dropped at the end.
    pg: Option<(String, String)>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .pool_max_idle_per_host(64)
        .build()
        .unwrap()
}

/// Swaps the database name in a server URL, keeping everything else (same as the test harness).
fn replace_database_in_url(server_url: &str, name: &str) -> String {
    let Some((scheme, rest)) = server_url.split_once("://") else {
        return format!("{server_url}/{name}");
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let suffix = tail.find(['?', '#']).map_or("", |at| &tail[at..]);
    format!("{scheme}://{authority}/{name}{suffix}")
}

async fn admin_exec(server_url: &str, sql: String) -> Result<(), sqlx::Error> {
    sqlx::any::install_default_drivers();
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(1)
        .connect(server_url)
        .await?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&pool).await?;
    pool.close().await;
    Ok(())
}

async fn start(args: &Args) -> Instance {
    let dir = tempfile::tempdir().unwrap();
    // Parsed rather than written out field by field, so a config field added later does not
    // break the benchmark. Everything that matters to it is then set explicitly.
    let mut config = logb::config::Config::parse_from(["logb"]);
    config.data_dir = dir.path().to_path_buf();
    config.bind = "127.0.0.1".into();
    config.port = 0;
    config.secure_cookie = "false".into();
    config.max_upload_mb = 50;
    config.max_import_mb = 4096;
    config.notify_url = None;
    config.telegram_bot_token = None;
    config.backup = None;
    config.backup_dir = None;
    config.restore = None;
    config.copy_to = None;
    config.login_max_attempts = 1000;
    config.db_pool_size = None;
    let mut pg = None;
    if let Some(server_url) = &args.postgres {
        let name = format!("logb_perf_{}", std::process::id());
        admin_exec(server_url, format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .await
            .expect("PostgreSQL server unreachable");
        admin_exec(server_url, format!("CREATE DATABASE {name}"))
            .await
            .expect("could not create the scratch database");
        config.database_url = Some(replace_database_in_url(server_url, &name));
        pg = Some((server_url.clone(), name));
    } else {
        config.database_url = None;
    }
    let (app, _state) = logb::build_with_state(config).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Instance {
        base: format!("http://{addr}/api"),
        _dir: dir,
        pg,
    }
}

impl Instance {
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
}

async fn expect_json(res: reqwest::Response, what: &str) -> Value {
    let status = res.status();
    let text = res.text().await.unwrap();
    assert!(status.is_success(), "{what} failed: {status} {text}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{what} answered {text}: {e}"))
}

async fn post(c: &reqwest::Client, url: String, body: Value) -> Value {
    let what = url.clone();
    expect_json(c.post(url).json(&body).send().await.unwrap(), &what).await
}

async fn get(c: &reqwest::Client, url: String) -> Value {
    let what = url.clone();
    expect_json(c.get(url).send().await.unwrap(), &what).await
}

// ---------------------------------------------------------------------------------------------
// Seeding.

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Car,
    EBike,
    Bike,
    Home,
    WaterMeter,
    Appliance,
    Tool,
    Body,
    Custom,
    Other,
}

struct Obj {
    id: i64,
    kind: Kind,
}

/// One activity to create: which object, and the body.
struct ActivityPlan {
    object: usize,
    body: Value,
}

const WORDS: &[&str] = &[
    "front", "rear", "left", "right", "filter", "brake", "chain", "tyre", "battery", "cable",
    "valve", "pump", "seal", "belt", "bearing", "lamp", "switch", "hose", "gasket", "screw",
    "garage", "warranty", "invoice", "noise", "leak", "crack", "rust", "cleaned", "adjusted",
    "replaced", "checked", "Bäckerei", "Müller", "Straße", "winter", "summer", "spring",
];
const TAGS: &[&str] = &[
    "diy", "workshop", "warranty", "urgent", "seasonal", "recurring", "insurance", "tax",
];

fn sentence(rng: &mut Rng, words: usize) -> String {
    (0..words).map(|_| *rng.pick(WORDS)).collect::<Vec<_>>().join(" ")
}

fn tags(rng: &mut Rng) -> Vec<&'static str> {
    let mut t: Vec<&str> = (0..rng.below(3)).map(|_| *rng.pick(TAGS)).collect();
    t.sort();
    t.dedup();
    t
}

fn date_of(day: i64) -> String {
    let base = chrono::NaiveDate::from_ymd_opt(2014, 1, 1).unwrap();
    (base + chrono::Duration::days(day)).to_string()
}

/// A small, unique JPEG: a gradient whose colours depend on `n`, so every file has its own
/// hash and gets its own thumbnail.
fn jpeg(n: u64) -> Vec<u8> {
    let (w, h) = (160u32, 120u32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let v = n.wrapping_mul(2654435761);
        image::Rgb([
            ((x + (v as u32)) % 256) as u8,
            ((y + (v >> 8) as u32) % 256) as u8,
            (((x ^ y) + (v >> 16) as u32) % 256) as u8,
        ])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .unwrap();
    out.into_inner()
}

async fn run_parallel<T: Send + 'static, R: Send + 'static, F, Fut>(
    items: Vec<T>,
    concurrency: usize,
    f: F,
) -> Vec<R>
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = R> + Send,
{
    // Chunks by index so the results come back in the order the items went in.
    let n = items.len();
    let queue = Arc::new(tokio::sync::Mutex::new(
        items.into_iter().enumerate().collect::<Vec<_>>().into_iter(),
    ));
    let f = Arc::new(f);
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..concurrency.max(1) {
        let queue = queue.clone();
        let f = f.clone();
        set.spawn(async move {
            let mut out = Vec::new();
            loop {
                let next = queue.lock().await.next();
                let Some((i, item)) = next else { break };
                out.push((i, f(item).await));
            }
            out
        });
    }
    let mut all: Vec<Option<R>> = (0..n).map(|_| None).collect();
    while let Some(done) = set.join_next().await {
        for (i, r) in done.unwrap() {
            all[i] = Some(r);
        }
    }
    all.into_iter().map(Option::unwrap).collect()
}

struct Seeded {
    busy_object: i64,
    home_with_contents: i64,
    ebike: i64,
    thumb_file: i64,
}

async fn seed(inst: &Instance, c: &reqwest::Client, args: &Args) -> Seeded {
    let mut rng = Rng(0x106B_2026_0929);
    let today = chrono::Local::now().date_naive();
    let today_day = (today - chrono::NaiveDate::from_ymd_opt(2014, 1, 1).unwrap()).num_days();

    // Custom types.
    let mut custom = Vec::new();
    for (name, icon) in [("Boat", "object"), ("Camera kit", "camera"), ("Drone", "object"),
        ("Documents", "document"), ("Trailer", "car")]
    {
        let t = post(c, inst.url("/types"), json!({
            "name": name, "icon": icon,
            "categories": ["maintenance", "repair", "purchase", "inspection", "other"],
            "counter_unit": null,
        })).await;
        custom.push(t["key"].as_str().unwrap().to_string());
    }

    // Objects: a fixed mix, homes first so appliances and tools can sit inside them.
    let mix: &[(Kind, u64)] = &[
        (Kind::Car, 12), (Kind::EBike, 8), (Kind::Bike, 8), (Kind::Home, 6),
        (Kind::WaterMeter, 4), (Kind::Appliance, 22), (Kind::Tool, 18), (Kind::Body, 3),
        (Kind::Custom, 9), (Kind::Other, 10),
    ];
    let total_weight: u64 = mix.iter().map(|m| m.1).sum();
    let mut kinds: Vec<Kind> = (0..args.objects)
        .map(|_| {
            let mut r = rng.below(total_weight);
            for (k, w) in mix {
                if r < *w {
                    return *k;
                }
                r -= w;
            }
            Kind::Other
        })
        .collect();
    // At least one of the kinds the timed endpoints need.
    for (i, k) in [Kind::Car, Kind::Home, Kind::EBike, Kind::WaterMeter].into_iter().enumerate() {
        if i < kinds.len() {
            kinds[i] = k;
        }
    }
    kinds.sort_by_key(|k| *k != Kind::Home);

    let mut objects: Vec<Obj> = Vec::new();
    let mut homes: Vec<i64> = Vec::new();
    for (i, kind) in kinds.into_iter().enumerate() {
        let name = format!("{} {}", match kind {
            Kind::Car => "Car", Kind::EBike => "E-bike", Kind::Bike => "Bike",
            Kind::Home => "House", Kind::WaterMeter => "Water meter", Kind::Appliance => "Washer",
            Kind::Tool => "Drill", Kind::Body => "Me", Kind::Custom => "Boat", Kind::Other => "Thing",
        }, i);
        let mut body = json!({
            "name": name, "description": sentence(&mut rng, 6),
            "purchase_date": date_of(rng.range(0, 3000)),
            "purchase_price_cents": rng.range(1_000, 3_000_000),
            "tags": tags(&mut rng),
            "archived": i > 20 && rng.chance(8),
        });
        let m = body.as_object_mut().unwrap();
        let (ty, extra): (&str, Value) = match kind {
            Kind::Car => ("car", json!({"counter_unit": "km", "fuel_unit": "l", "fuel_capacity_milli": 50_000})),
            Kind::EBike => ("e_bike", json!({"counter_unit": "km", "fuel_unit": "kwh", "energy_price_milli": 30_000})),
            Kind::Bike => ("bike", json!({"counter_unit": "km"})),
            Kind::Home => ("home", json!({"resource_unit": "kwh", "resource_kind": "electricity", "measurement_mode": "usage"})),
            Kind::WaterMeter => ("home", json!({"resource_unit": "m3", "resource_kind": "water", "measurement_mode": "meter"})),
            Kind::Appliance => ("appliance", json!({})),
            Kind::Tool => ("tool", json!({"counter_unit": "h"})),
            Kind::Body => ("body", json!({})),
            Kind::Custom => (rng.pick(&custom).as_str(), json!({})),
            Kind::Other => ("other", json!({})),
        };
        m.insert("type".into(), json!(ty));
        for (k, v) in extra.as_object().unwrap() {
            m.insert(k.clone(), v.clone());
        }
        // Appliances and tools usually sit inside a home; a few homes' contents nest twice.
        if matches!(kind, Kind::Appliance | Kind::Tool | Kind::WaterMeter) && !homes.is_empty() && rng.chance(80) {
            m.insert("parent_id".into(), json!(*rng.pick(&homes)));
        }
        let o = post(c, inst.url("/objects"), body).await;
        let id = o["id"].as_i64().unwrap();
        if kind == Kind::Home {
            homes.push(id);
        }
        objects.push(Obj { id, kind });
    }

    // Activities: skewed so a handful of objects carry long histories, as real ones do. The
    // first object of each of these kinds is one of them, and the timed per-object endpoints
    // below ask about exactly those.
    let first_of = |kind: Kind| objects.iter().position(|o| o.kind == kind);
    let long_histories: Vec<usize> = [Kind::Car, Kind::Home, Kind::EBike, Kind::WaterMeter]
        .into_iter()
        .filter_map(first_of)
        .collect();
    let weights: Vec<u64> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let base = match o.kind {
                Kind::Car | Kind::EBike | Kind::WaterMeter | Kind::Body => 12,
                Kind::Home => 8,
                _ => 2,
            };
            if long_histories.contains(&i) { base * 25 } else { base }
        })
        .collect();
    let wsum: u64 = weights.iter().sum();
    let mut counts = vec![0usize; objects.len()];
    for _ in 0..args.activities {
        let mut r = rng.below(wsum);
        for (i, w) in weights.iter().enumerate() {
            if r < *w {
                counts[i] += 1;
                break;
            }
            r -= w;
        }
    }
    let mut plans: Vec<ActivityPlan> = Vec::with_capacity(args.activities);
    let mut rare_left = 3;
    for (oi, o) in objects.iter().enumerate() {
        let n = counts[oi] as i64;
        if n == 0 {
            continue;
        }
        // Spread over up to ten years, ending today; counters and meters only ever go up.
        let span = today_day.min(3650);
        let mut days: Vec<i64> = (0..n).map(|_| today_day - rng.range(0, span)).collect();
        days.sort();
        let mut counter = rng.range(0, 5_000);
        let mut meter = rng.range(0, 100_000);
        for day in days {
            let date = date_of(day);
            counter += rng.range(5, 400);
            meter += rng.range(50, 800);
            let mut notes = if rng.chance(40) { sentence(&mut rng, 12) } else { String::new() };
            if rare_left > 0 && rng.chance(1) {
                notes.push_str(" zebrafish");
                rare_left -= 1;
            }
            let cost = rng.chance(60).then(|| rng.range(100, 90_000));
            let t = tags(&mut rng);
            let body = match o.kind {
                Kind::Car | Kind::EBike | Kind::Bike => match rng.below(10) {
                    0..=3 if o.kind != Kind::Bike => {
                        let mut b = json!({"date": date, "category": "fuel", "title": "",
                            "counter_value": counter, "cost_cents": cost.unwrap_or(4_000),
                            "quantity_milli": rng.range(5_000, 45_000), "notes": notes, "tags": t});
                        if o.kind == Kind::Car && rng.chance(50) {
                            b["fuel_level_pct"] = json!(rng.range(10, 100));
                        }
                        b
                    }
                    4 | 5 => {
                        let start = counter - rng.range(1, 5);
                        json!({"date": date, "category": "trip", "title": "", "counter_value": counter,
                            "start_counter": start, "from_place": "Home", "to_place": *rng.pick(&["Work", "Shop", "Lake", "Bäckerei Müller"]),
                            "duration_minutes": rng.range(5, 120), "notes": notes, "tags": t})
                    }
                    6 => json!({"date": date, "category": "reading", "title": "Reading", "counter_value": counter, "notes": notes, "tags": t}),
                    _ => json!({"date": date, "category": *rng.pick(&["maintenance", "repair", "inspection"]),
                        "title": *rng.pick(&["Oil change", "Brake pads", "Tyre swap", "Chain oil", "Service", "TÜV"]),
                        "counter_value": counter, "cost_cents": cost, "notes": notes, "tags": t}),
                },
                Kind::WaterMeter => json!({"date": date, "category": "usage", "title": "Meter reading",
                    "meter_reading_milli": meter, "cost_cents": cost, "notes": notes, "tags": t}),
                Kind::Home if rng.chance(50) => json!({"date": date, "category": "usage", "title": "Electricity",
                    "quantity_milli": rng.range(50_000, 600_000), "cost_cents": cost, "notes": notes, "tags": t}),
                Kind::Body => json!({"date": date, "category": "weight", "title": "Weight",
                    "weight_grams": rng.range(70_000, 90_000), "notes": notes, "tags": t}),
                Kind::Tool => json!({"date": date, "category": *rng.pick(&["maintenance", "repair", "other"]),
                    "title": *rng.pick(&["Blade change", "Oil the gearbox", "Cleaned", "New cable"]),
                    "counter_value": counter / 100, "cost_cents": cost, "notes": notes, "tags": t}),
                _ => json!({"date": date, "category": *rng.pick(&["maintenance", "repair", "purchase", "inspection", "other"]),
                    "title": *rng.pick(&["Descaled", "Filter swap", "Repair", "Spare part", "Oil the hinge", "Inspection"]),
                    "cost_cents": cost, "notes": notes, "tags": t}),
            };
            plans.push(ActivityPlan { object: oi, body });
        }
    }
    let object_ids: Arc<Vec<i64>> = Arc::new(objects.iter().map(|o| o.id).collect());
    let (cc, base) = (c.clone(), inst.base.clone());
    let started = Instant::now();
    let created: Vec<(usize, i64)> = run_parallel(plans, args.seed_concurrency, move |p| {
        let (c, base, ids) = (cc.clone(), base.clone(), object_ids.clone());
        async move {
            let a = post(&c, format!("{base}/objects/{}/activities", ids[p.object]), p.body).await;
            (p.object, a["id"].as_i64().unwrap())
        }
    })
    .await;
    eprintln!("  {} activities in {:.1}s", created.len(), started.elapsed().as_secs_f64());

    // Attachments: most on activities, the rest on objects directly.
    let jobs: Vec<(i64, Option<i64>, u64)> = (0..args.attachments as u64)
        .map(|n| {
            if !created.is_empty() && rng.chance(75) {
                let (oi, aid) = *rng.pick(&created);
                (objects[oi].id, Some(aid), n)
            } else {
                (rng.pick(&objects).id, None, n)
            }
        })
        .collect();
    let (cc, base) = (c.clone(), inst.base.clone());
    let started = Instant::now();
    let files: Vec<i64> = run_parallel(jobs, args.seed_concurrency, move |(oid, aid, n)| {
        let (c, base) = (cc.clone(), base.clone());
        async move {
            let mut form = Form::new()
                .part("file", Part::bytes(jpeg(n)).file_name(format!("photo-{n}.jpg")).mime_str("image/jpeg").unwrap())
                .text("caption", format!("photo {n}"));
            if let Some(aid) = aid {
                form = form.text("activity_id", aid.to_string());
            }
            let url = format!("{base}/objects/{oid}/attachments");
            let a = expect_json(c.post(&url).multipart(form).send().await.unwrap(), &url).await;
            a["file_id"].as_i64().unwrap_or(0)
        }
    })
    .await;
    eprintln!("  {} attachments in {:.1}s", files.len(), started.elapsed().as_secs_f64());

    // Reminders: about one per two objects, some due, some by counter, some done.
    for o in &objects {
        if !rng.chance(50) {
            continue;
        }
        let with_counter = matches!(o.kind, Kind::Car | Kind::EBike | Kind::Bike) && rng.chance(50);
        let body = if with_counter {
            json!({"title": "Service", "due_counter": rng.range(1_000, 400_000), "repeat_counter": 15_000})
        } else {
            let due = today + chrono::Duration::days(rng.range(-60, 400));
            json!({"title": *rng.pick(&["Inspection", "Replace filter", "Insurance", "Check pressure"]),
                "due_date": due.to_string(), "repeat_months": *rng.pick(&[1i64, 3, 6, 12, 24])})
        };
        let r = post(c, inst.url(&format!("/objects/{}/reminders", o.id)), body).await;
        if rng.chance(20) {
            post(c, inst.url(&format!("/reminders/{}/done", r["id"])), json!({})).await;
        }
    }

    // The car with the long history: fuel, trips, readings and repairs, like a real one.
    let busy_object = objects.iter().find(|o| o.kind == Kind::Car).map_or(objects[0].id, |o| o.id);
    let home_with_contents = objects.iter().find(|o| o.kind == Kind::Home).map_or(busy_object, |o| o.id);
    let ebike = objects.iter().find(|o| o.kind == Kind::EBike).map_or(busy_object, |o| o.id);
    let thumb_file = files.iter().copied().find(|f| *f > 0).unwrap_or(0);
    Seeded { busy_object, home_with_contents, ebike, thumb_file }
}

// ---------------------------------------------------------------------------------------------
// Timing.

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    // Nearest-rank.
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

fn summarise(name: &str, mut samples: Vec<f64>, bytes: u64) -> Measurement {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if samples.len() % 2 == 1 {
        samples[samples.len() / 2]
    } else {
        (samples[samples.len() / 2 - 1] + samples[samples.len() / 2]) / 2.0
    };
    Measurement {
        name: name.to_string(),
        runs: samples.len(),
        median_ms: median,
        p95_ms: percentile(&samples, 95.0),
        min_ms: samples[0],
        max_ms: *samples.last().unwrap(),
        bytes,
    }
}

/// Sends one GET and reads the whole body, answering the elapsed milliseconds and body size.
async fn time_get(c: &reqwest::Client, url: &str) -> (f64, u64) {
    let started = Instant::now();
    let res = c
        .get(url)
        .header("accept-encoding", ACCEPT_ENCODING)
        .send()
        .await
        .unwrap();
    let status = res.status();
    let body = res.bytes().await.unwrap();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    assert!(status.is_success(), "GET {url} failed: {status}");
    (ms, body.len() as u64)
}

async fn bench_get(c: &reqwest::Client, name: &str, url: &str, warmup: usize, runs: usize) -> Measurement {
    for _ in 0..warmup {
        time_get(c, url).await;
    }
    let mut samples = Vec::with_capacity(runs);
    let mut bytes = 0;
    for _ in 0..runs {
        let (ms, b) = time_get(c, url).await;
        samples.push(ms);
        bytes = b;
    }
    let m = summarise(name, samples, bytes);
    eprintln!("  {:<28} median {:>9.2} ms   p95 {:>9.2} ms", m.name, m.median_ms, m.p95_ms);
    m
}

fn print_table(results: &[Measurement]) {
    println!("{:<28} {:>5} {:>11} {:>11} {:>11} {:>11}", "endpoint", "runs", "median ms", "p95 ms", "min ms", "bytes");
    println!("{}", "-".repeat(82));
    for m in results {
        println!(
            "{:<28} {:>5} {:>11.2} {:>11.2} {:>11.2} {:>11}",
            m.name, m.runs, m.median_ms, m.p95_ms, m.min_ms, m.bytes
        );
    }
}

fn compare(before: &std::path::Path, after: &std::path::Path) {
    let read = |p: &std::path::Path| -> Report {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    };
    let (a, b) = (read(before), read(after));
    println!("before: {} {} {} ({} objects, {} activities, {} attachments)", a.backend, a.git, a.date, a.objects, a.activities, a.attachments);
    println!("after:  {} {} {} ({} objects, {} activities, {} attachments)", b.backend, b.git, b.date, b.objects, b.activities, b.attachments);
    println!();
    println!("{:<28} {:>11} {:>11} {:>8}   {:>11} {:>11} {:>8}", "endpoint", "median a", "median b", "delta", "p95 a", "p95 b", "delta");
    println!("{}", "-".repeat(98));
    let pct = |x: f64, y: f64| if x > 0.0 { format!("{:+.0}%", (y - x) / x * 100.0) } else { "-".into() };
    for m in &a.results {
        match b.results.iter().find(|n| n.name == m.name) {
            Some(n) => println!(
                "{:<28} {:>11.2} {:>11.2} {:>8}   {:>11.2} {:>11.2} {:>8}",
                m.name, m.median_ms, n.median_ms, pct(m.median_ms, n.median_ms), m.p95_ms, n.p95_ms, pct(m.p95_ms, n.p95_ms)
            ),
            None => println!("{:<28} {:>11.2} {:>11} (only in before)", m.name, m.median_ms, "-"),
        }
    }
    for n in b.results.iter().filter(|n| !a.results.iter().any(|m| m.name == n.name)) {
        println!("{:<28} {:>11} {:>11.2} (only in after)", n.name, "-", n.median_ms);
    }
}

fn git_describe() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Some(files) = &args.compare {
        compare(&files[0], &files[1]);
        return;
    }
    if cfg!(debug_assertions) {
        eprintln!("warning: this is a debug build; use `cargo run --release --example perf` for real numbers");
    }
    // Quiet unless asked: the app logs a line per request at info.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("LOGB_PERF_LOG").unwrap_or_else(|_| "warn".into()),
        ))
        .with_writer(std::io::stderr)
        .init();

    let inst = start(&args).await;
    let backend = if inst.pg.is_some() { "postgres" } else { "sqlite" };
    eprintln!("seeding {backend}: {} objects, {} activities, {} attachments", args.objects, args.activities, args.attachments);
    let c = client();
    let res = c
        .post(inst.url("/auth/setup"))
        .json(&json!({"username": "perf", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "setup failed: {}", res.text().await.unwrap());
    let seed_started = Instant::now();
    let s = seed(&inst, &c, &args).await;
    let seed_seconds = seed_started.elapsed().as_secs_f64();
    eprintln!("seeded in {seed_seconds:.1}s");

    let boot = get(&c, inst.url("/sync/bootstrap")).await;
    let epoch = boot["epoch"].as_str().unwrap().to_string();
    let seq = boot["seq"].as_i64().unwrap();
    let today = chrono::Local::now().date_naive();
    let (busy, home, ebike) = (s.busy_object, s.home_with_contents, s.ebike);

    let gets: Vec<(&str, String)> = vec![
        ("objects_all", "/objects?all=true&archived=false".into()),
        ("objects_all_archived", "/objects?all=true&archived=true".into()),
        ("objects_roots", "/objects".into()),
        ("object_detail", format!("/objects/{busy}")),
        ("object_children", format!("/objects?parent_id={home}&archived=false")),
        ("timeline_first_page", format!("/objects/{busy}/activities?limit=100&offset=0")),
        ("timeline_deep_page", format!("/objects/{busy}/activities?limit=100&offset=500")),
        ("timeline_category", format!("/objects/{busy}/activities?limit=100&offset=0&category=fuel")),
        ("last_done", format!("/objects/{busy}/last-done")),
        ("recent_titles", format!("/objects/{busy}/recent-titles")),
        ("attachments_list", format!("/objects/{busy}/attachments")),
        ("trips_summary", format!("/objects/{busy}/trips/summary?today={today}")),
        ("energy", format!("/objects/{ebike}/energy")),
        ("insights", format!("/objects/{busy}/insights")),
        ("insights_contents", format!("/objects/{home}/insights?contents=true")),
        ("usage", format!("/objects/{busy}/usage")),
        ("search_common", format!("/search?q={COMMON_TERM}")),
        ("search_rare", format!("/search?q={RARE_TERM}")),
        ("stats", "/stats".into()),
        ("stats_year", format!("/stats?year={}", chrono::Datelike::year(&today) - 1)),
        ("stats_energy", "/stats/energy".into()),
        ("stats_fuel", "/stats/fuel".into()),
        ("stats_water", "/stats/water".into()),
        ("reminders_due", "/reminders/due?within_days=30".into()),
        ("tags", "/tags".into()),
        ("types", "/types".into()),
        ("auth_me", "/auth/me".into()),
        ("thumb", format!("/files/{}/thumb", s.thumb_file)),
        ("sync_bootstrap", "/sync/bootstrap".into()),
        ("sync_pull_first", "/sync/pull?since=0&limit=500".into()),
        ("sync_pull_tail", format!("/sync/pull?since={}&epoch={epoch}&limit=500", (seq - 200).max(1))),
    ];
    eprintln!("timing ({} warm-up, {} runs each)", args.warmup, args.runs);
    let mut results = Vec::new();
    for (name, path) in &gets {
        results.push(bench_get(&c, name, &inst.url(path), args.warmup, args.runs).await);
    }

    if !args.skip_heavy {
        let export_url = inst.url("/export");
        results.push(bench_get(&c, "export", &export_url, 1, args.heavy_runs).await);
        let archive = c.get(&export_url).send().await.unwrap().bytes().await.unwrap();
        eprintln!("  export archive is {:.1} MB", archive.len() as f64 / 1e6);
        // Each import goes into a user of its own, so every run starts from the same empty
        // account rather than on top of the previous run's copy.
        let mut samples = Vec::new();
        for i in 0..args.heavy_runs {
            let name = format!("importer{i}");
            post(&c, inst.url("/users"), json!({"username": name, "password": PASSWORD})).await;
            let u = client();
            post(&u, inst.url("/auth/login"), json!({"username": name, "password": PASSWORD})).await;
            let started = Instant::now();
            let res = u
                .post(inst.url("/import"))
                .header("content-type", "application/zip")
                .body(archive.clone())
                .send()
                .await
                .unwrap();
            let status = res.status();
            let text = res.text().await.unwrap();
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            assert!(status.is_success(), "import failed: {status} {text}");
            samples.push(ms);
        }
        let m = summarise("import", samples, archive.len() as u64);
        eprintln!("  {:<28} median {:>9.2} ms   p95 {:>9.2} ms", m.name, m.median_ms, m.p95_ms);
        results.push(m);
    }

    println!();
    println!("LogB perf -- {backend}, {} objects, {} activities, {} attachments, git {}", args.objects, args.activities, args.attachments, git_describe());
    print_table(&results);

    if let Some(path) = &args.json {
        let report = Report {
            backend: backend.into(),
            git: git_describe(),
            date: chrono::Local::now().to_rfc3339(),
            objects: args.objects,
            activities: args.activities,
            attachments: args.attachments,
            seed_seconds,
            results,
        };
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap() + "\n").unwrap();
        eprintln!("wrote {}", path.display());
    }

    if let Some((server_url, name)) = &inst.pg {
        // The app's pool still holds connections, hence FORCE.
        if let Err(e) = admin_exec(server_url, format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await {
            eprintln!("could not drop the scratch database {name}: {e}");
        }
    }
    // Leaves the server task and pool behind; the process is about to end anyway.
    tokio::time::sleep(Duration::from_millis(10)).await;
}
