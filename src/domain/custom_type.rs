//! A user's own object types ("E-scooter", "Boat"), next to the nine built-in ones.
//!
//! Pure rules only: what a valid name, icon, category list and counter unit are. The database
//! half -- uniqueness among a user's types, and whether an object may use a key -- lives in
//! `api::types` and `object_type::is_valid_for_user`, because both need the caller's rows.

use crate::api::activities::CATEGORIES;

pub const MAX_NAME_CHARS: usize = 40;

/// An object's `type` is either a built-in key or this prefix plus the type's `client_uuid`.
/// The uuid rather than the integer id, so a type and the objects using it can be created
/// offline together and replayed in order.
pub const CUSTOM_PREFIX: &str = "custom:";

/// The icons a custom type may use: `IconName` in `frontend/src/lib/Icon.svelte` minus the
/// icons that only mean something as interface controls (back, settings, search, ...). Kept
/// here as well because the server must refuse an icon the client cannot draw.
pub const CUSTOM_TYPE_ICONS: &[&str] = &[
    "document", "camera", "car", "e-bike", "bike", "motorcycle", "home", "appliance", "tool", "body",
    "object", "box",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInput {
    pub name: String,
    pub icon: String,
    pub categories: Vec<String>,
    pub counter_unit: Option<String>,
}

/// Trims the name and checks every field. Categories keep their first-seen order with later
/// duplicates dropped, and `other` is appended when missing: every entry form needs a category
/// that fits anything. `Err` is the sentence a 400 answers with.
pub fn normalize(input: TypeInput) -> Result<TypeInput, String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("name is required".into());
    }
    // Characters, not bytes: "Gerät" is five characters however it is encoded.
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("name can be at most {MAX_NAME_CHARS} characters long"));
    }
    if !CUSTOM_TYPE_ICONS.contains(&input.icon.as_str()) {
        return Err(format!("icon must be one of {}", CUSTOM_TYPE_ICONS.join(", ")));
    }
    let categories = normalize_categories(input.categories)?;
    if let Some(unit) = &input.counter_unit {
        if !matches!(unit.as_str(), "km" | "mi" | "h") {
            return Err("counter_unit must be km, mi, h or null".into());
        }
    }
    Ok(TypeInput { name, icon: input.icon, categories, counter_unit: input.counter_unit })
}

/// The category half of `normalize`, on its own so sync can log a pushed `categories` value in
/// its stored spelling without a whole type to hand.
pub fn normalize_categories(input: Vec<String>) -> Result<Vec<String>, String> {
    if input.is_empty() {
        return Err("categories must name at least one category".into());
    }
    let mut categories: Vec<String> = Vec::with_capacity(input.len() + 1);
    for category in input {
        if !CATEGORIES.contains(&category.as_str()) {
            return Err(format!("category must be one of {}", CATEGORIES.join(", ")));
        }
        if !categories.contains(&category) {
            categories.push(category);
        }
    }
    if !categories.iter().any(|c| c == "other") {
        categories.push("other".into());
    }
    Ok(categories)
}

/// `"custom:<uuid>"` -> `Some("<uuid>")`; a built-in key, or a bare prefix, -> `None`.
pub fn custom_uuid(type_key: &str) -> Option<&str> {
    type_key.strip_prefix(CUSTOM_PREFIX).filter(|uuid| !uuid.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, icon: &str, categories: &[&str], unit: Option<&str>) -> TypeInput {
        TypeInput {
            name: name.into(),
            icon: icon.into(),
            categories: categories.iter().map(|c| c.to_string()).collect(),
            counter_unit: unit.map(str::to_string),
        }
    }

    #[test]
    fn the_name_is_trimmed_and_limited_in_characters_not_bytes() {
        let out = normalize(input("  E-scooter ", "e-bike", &["repair"], None)).unwrap();
        assert_eq!(out.name, "E-scooter");
        assert!(normalize(input(&"é".repeat(MAX_NAME_CHARS), "box", &["repair"], None)).is_ok());
        assert!(normalize(input(&"x".repeat(MAX_NAME_CHARS + 1), "box", &["repair"], None)).unwrap_err().contains("40"));
    }

    #[test]
    fn an_empty_name_is_refused() {
        assert!(normalize(input("   ", "box", &["repair"], None)).is_err());
    }

    #[test]
    fn an_unknown_icon_is_refused() {
        assert!(normalize(input("Boat", "settings", &["repair"], None)).is_err());
        assert!(normalize(input("Boat", "rocket", &["repair"], None)).is_err());
    }

    #[test]
    fn unknown_or_empty_categories_are_refused() {
        assert!(normalize(input("Boat", "box", &["repair", "sailing"], None)).is_err());
        assert!(normalize(input("Boat", "box", &[], None)).is_err());
    }

    #[test]
    fn other_is_added_and_duplicates_removed_in_order() {
        let out = normalize(input("Boat", "box", &["fuel", "repair", "fuel"], None)).unwrap();
        assert_eq!(out.categories, ["fuel", "repair", "other"]);
        let out = normalize(input("Boat", "box", &["other", "repair"], None)).unwrap();
        assert_eq!(out.categories, ["other", "repair"]);
    }

    #[test]
    fn the_counter_unit_is_km_mi_h_or_none() {
        for unit in [Some("km"), Some("mi"), Some("h"), None] {
            assert_eq!(normalize(input("Boat", "box", &["repair"], unit)).unwrap().counter_unit.as_deref(), unit);
        }
        assert!(normalize(input("Boat", "box", &["repair"], Some("nm"))).is_err());
    }

    #[test]
    fn custom_uuid_reads_only_custom_keys() {
        assert_eq!(custom_uuid("custom:abc"), Some("abc"));
        assert_eq!(custom_uuid("car"), None);
        assert_eq!(custom_uuid("custom:"), None);
    }
}
