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
    let documented = find_enum_containing(&doc, "maintenance")
        .expect("the document should describe activity categories");
    let real: Vec<String> = logb::api::activities::CATEGORIES.iter().map(|c| c.to_string()).collect();
    assert_eq!(documented, real, "docs/openapi.json disagrees with api::activities::CATEGORIES");
}

/// The object types the document lists must be the ones the API accepts.
///
/// `Category` was guarded from the day it was found stale; its sibling `ObjectType`, added a
/// release later, was not -- removing `body` from it failed nothing. A type missing from the
/// spec is a client that never offers it, and one invented in the spec is a client whose POST
/// is rejected with 400.
///
/// The built-in keys are `examples`, not an `enum`, since a user's own `custom:<uuid>` types are
/// valid too -- but they must still be exactly the built-in list, in order.
#[test]
fn the_documented_object_types_are_the_real_ones() {
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string("docs/openapi.json").expect("docs/openapi.json should be readable"),
    )
    .expect("docs/openapi.json should be valid JSON");
    let documented: Vec<String> = doc["components"]["schemas"]["ObjectType"]["examples"]
        .as_array()
        .expect("the document should describe object types")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let real: Vec<String> = logb::object_type::OBJECT_TYPES.iter().map(|t| t.to_string()).collect();
    assert_eq!(documented, real, "docs/openapi.json disagrees with object_type::OBJECT_TYPES");
}

/// The first `enum` whose members include `marker` -- how a list is found without hard-coding
/// where in the document it happens to live.
fn find_enum_containing(node: &Value, marker: &str) -> Option<Vec<String>> {
    match node {
        Value::Object(map) => {
            if let Some(Value::Array(values)) = map.get("enum") {
                let members: Vec<String> =
                    values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                if members.iter().any(|m| m == marker) {
                    return Some(members);
                }
            }
            map.values().find_map(|v| find_enum_containing(v, marker))
        }
        Value::Array(items) => items.iter().find_map(|v| find_enum_containing(v, marker)),
        _ => None,
    }
}

/// `CustomTypeInput.icon` is the spec's copy of the icons the server accepts. The frontend's copy
/// is compared with the server's in `frontend/tests/icons.test.ts`; this closes the triangle, so
/// a spec still offering an icon the server refuses (it once listed `box`) fails here.
#[test]
fn the_spec_offers_exactly_the_icons_an_own_type_may_have() {
    let raw = std::fs::read_to_string("docs/openapi.json").unwrap();
    let spec: Value = serde_json::from_str(&raw).unwrap();
    let documented: Vec<&str> = spec["components"]["schemas"]["CustomTypeInput"]["properties"]["icon"]["enum"]
        .as_array()
        .expect("CustomTypeInput.icon should be an enum")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(documented, logb::domain::custom_type::CUSTOM_TYPE_ICONS);
}
