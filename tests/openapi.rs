//! Keeps `docs/openapi.json` honest.
//!
//! The spec exists so a second client -- a phone app, a script -- can be written against
//! something other than the source. A spec nobody checks drifts within a release or two and is
//! then worse than none, because it is believed. So rather than trusting a comment, this reads
//! the routes the router actually declares and compares them with the paths the document
//! describes, in both directions.
//!
//! It reads the source rather than the `Router`, because axum exposes no way to enumerate what
//! it has been given. That makes this a lexical check: it verifies the two lists agree, not
//! that each handler behaves as documented. The integration tests either side of it cover
//! behaviour; this covers drift.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Every `.route("...", get(..).post(..))` declared under `src/api/`, as (path, method) pairs.
fn declared_routes() -> BTreeMap<String, BTreeSet<String>> {
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let dir = std::fs::read_dir("src/api").expect("src/api should be readable from the crate root");
    for entry in dir {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        for (i, _) in src.match_indices(".route(") {
            let rest = &src[i + ".route(".len()..];
            // `.route("/objects/{id}", get(read).patch(update))` -- the literal, then the
            // method calls up to the closing paren of the `route(` call itself.
            let quote = rest.find('"').expect("a route literal");
            let end = rest[quote + 1..].find('"').expect("an unterminated route literal");
            let route = &rest[quote + 1..quote + 1 + end];
            let tail = &rest[quote + 1 + end..];
            let close = tail.find("))").map(|n| n + 2).unwrap_or(tail.len());
            let methods = &tail[..close];
            let entry = found.entry(route.to_string()).or_default();
            for m in ["get", "post", "patch", "put", "delete"] {
                // `patch(update)` and `axum::routing::patch(update)` both count; `delete` as a
                // handler NAME (`.delete(delete)`) is matched by the same needle, which is
                // harmless because the method is `delete` in that case anyway.
                if methods.contains(&format!("{m}(")) {
                    entry.insert(m.to_string());
                }
            }
        }
    }
    found
}

fn documented_routes() -> BTreeMap<String, BTreeSet<String>> {
    let raw = std::fs::read_to_string("docs/openapi.json").expect("docs/openapi.json should exist");
    let spec: Value = serde_json::from_str(&raw).expect("docs/openapi.json should be valid JSON");
    spec["paths"]
        .as_object()
        .expect("the spec should have a `paths` object")
        .iter()
        .map(|(path, ops)| {
            let methods = ops.as_object().unwrap().keys()
                .filter(|k| !k.starts_with('x') && *k != "parameters")
                .cloned()
                .collect();
            (path.clone(), methods)
        })
        .collect()
}

#[test]
fn the_spec_describes_exactly_the_routes_the_api_serves() {
    let declared = declared_routes();
    let documented = documented_routes();

    assert!(!declared.is_empty(), "no routes were found; the scan above has stopped working");

    let undocumented: Vec<_> = declared.keys().filter(|p| !documented.contains_key(*p)).collect();
    assert!(
        undocumented.is_empty(),
        "these routes exist but are not in docs/openapi.json: {undocumented:?}",
    );

    let imaginary: Vec<_> = documented.keys().filter(|p| !declared.contains_key(*p)).collect();
    assert!(
        imaginary.is_empty(),
        "docs/openapi.json describes routes that no longer exist: {imaginary:?}",
    );

    for (path, methods) in &declared {
        assert_eq!(
            methods, &documented[path],
            "the methods on {path} differ between the router and docs/openapi.json",
        );
    }
}

/// A client author reads the spec to find out how to authenticate; getting that wrong is the
/// difference between an app that works and one that cannot log in at all.
#[test]
fn the_spec_states_how_to_authenticate() {
    let raw = std::fs::read_to_string("docs/openapi.json").unwrap();
    let spec: Value = serde_json::from_str(&raw).unwrap();

    let schemes = &spec["components"]["securitySchemes"];
    assert_eq!(schemes["sessionCookie"]["name"], "logb_session");
    assert_eq!(schemes["bearerToken"]["scheme"], "bearer");

    // Token management refuses bearer credentials on purpose, and the spec has to say so or an
    // app will be built around a call that always 401s.
    for path in ["/auth/tokens", "/auth/tokens/{id}"] {
        for (method, op) in spec["paths"][path].as_object().unwrap() {
            let security = op["security"].as_array()
                .unwrap_or_else(|| panic!("{method} {path} should state its own security"));
            let names: Vec<_> = security.iter().flat_map(|s| s.as_object().unwrap().keys()).collect();
            assert_eq!(names, vec!["sessionCookie"], "{method} {path} should be cookie-only");
        }
    }

    // And the routes that need nothing must say so, or a client will try to log in before it
    // can ask whether the instance even has a user yet.
    for path in ["/health", "/auth/status", "/auth/login", "/auth/setup"] {
        let op = spec["paths"][path].as_object().unwrap().values().next().unwrap();
        assert_eq!(op["security"], serde_json::json!([]), "{path} should be documented as public");
    }
}

/// The activity categories the document lists must be the ones the API accepts.
///
/// This enum was stale before anyone noticed: it named `insurance` and `tax`, which no version
/// of this app has ever accepted, and omitted `modification`, which it always has. A spec that
/// invents values is worse than one that omits them, because a second client written against it
/// sends something the server rejects.
#[test]
fn the_documented_activity_categories_are_the_real_ones() {
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string("docs/openapi.json").expect("docs/openapi.json should be readable"),
    )
    .expect("docs/openapi.json should be valid JSON");
    let documented = find_category_enum(&doc).expect("the document should describe activity categories");
    let real: Vec<String> = logb::api::activities::CATEGORIES.iter().map(|c| c.to_string()).collect();
    assert_eq!(documented, real, "docs/openapi.json disagrees with api::activities::CATEGORIES");
}

/// The first `enum` whose members include `maintenance` -- the activity category list.
fn find_category_enum(node: &Value) -> Option<Vec<String>> {
    match node {
        Value::Object(map) => {
            if let Some(Value::Array(values)) = map.get("enum") {
                let members: Vec<String> =
                    values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                if members.iter().any(|m| m == "maintenance") {
                    return Some(members);
                }
            }
            map.values().find_map(find_category_enum)
        }
        Value::Array(items) => items.iter().find_map(find_category_enum),
        _ => None,
    }
}
