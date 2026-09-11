//! What kind of thing an object is.
//!
//! The nine types are a closed set with a `CHECK` behind them, because behaviour keys off this
//! value: the icon a row shows, and which activity categories its form offers. A free-text
//! field cannot carry that -- `Fahrrad`, `bike` and a typo were three different values.

/// The nine types, in the order the picker offers them.
pub const OBJECT_TYPES: [&str; 9] = [
    "car", "e_bike", "bike", "motorcycle", "home", "appliance", "tool", "body", "other",
];

pub fn is_valid(t: &str) -> bool {
    OBJECT_TYPES.contains(&t)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Legacy {
    Mapped(&'static str),
    Unmapped,
}

/// Legacy free-text spellings, in both languages the interface speaks.
///
/// Matching is exact on the lowercased, trimmed text -- never substring. `Gravelbike Custom`
/// becoming a `bike` by accident is a silent mis-filing with no record that a guess was made;
/// landing on `other` with the words preserved is visible and fixed in one edit.
///
/// `geraet` and `koerper` sit beside `gerät` and `körper` on purpose: a phone keyboard set to
/// English has no umlauts, which is the likeliest way this data was typed in the first place.
///
/// `migrations/sqlite/0009_object_types.sql` repeats this table in SQL and
/// `tests/migration_object_types.rs` runs every word here through the real migration, so the
/// two cannot drift.
pub const LEGACY: [(&str, &str); 32] = [
    ("car", "car"), ("auto", "car"), ("pkw", "car"), ("wagen", "car"),
    ("e-bike", "e_bike"), ("ebike", "e_bike"), ("e bike", "e_bike"), ("pedelec", "e_bike"),
    ("bike", "bike"), ("fahrrad", "bike"), ("velo", "bike"), ("rad", "bike"),
    ("motorcycle", "motorcycle"), ("motorrad", "motorcycle"), ("motorbike", "motorcycle"),
    ("home", "home"), ("haus", "home"), ("wohnung", "home"), ("flat", "home"), ("apartment", "home"),
    ("appliance", "appliance"), ("gerät", "appliance"), ("geraet", "appliance"),
    ("haushaltsgerät", "appliance"),
    ("tool", "tool"), ("werkzeug", "tool"), ("maschine", "tool"),
    ("body", "body"), ("körper", "body"), ("koerper", "body"), ("health", "body"),
    ("gesundheit", "body"),
];

pub fn from_legacy(text: &str) -> Legacy {
    let key = text.trim().to_lowercase();
    match LEGACY.iter().find(|(word, _)| *word == key) {
        Some((_, ty)) => Legacy::Mapped(ty),
        None => Legacy::Unmapped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_words_map_in_both_languages() {
        assert_eq!(from_legacy("car"), Legacy::Mapped("car"));
        assert_eq!(from_legacy("Auto"), Legacy::Mapped("car"));
        assert_eq!(from_legacy("  PKW  "), Legacy::Mapped("car"));
        assert_eq!(from_legacy("fahrrad"), Legacy::Mapped("bike"));
        assert_eq!(from_legacy("Werkzeug"), Legacy::Mapped("tool"));
        assert_eq!(from_legacy("körper"), Legacy::Mapped("body"));
        // A phone keyboard without umlauts is the likeliest source of this data.
        assert_eq!(from_legacy("Koerper"), Legacy::Mapped("body"));
        assert_eq!(from_legacy("GERAET"), Legacy::Mapped("appliance"));
        assert_eq!(from_legacy("Haushaltsgerät"), Legacy::Mapped("appliance"));
        assert_eq!(from_legacy("Apartment"), Legacy::Mapped("home"));
        assert_eq!(from_legacy("Maschine"), Legacy::Mapped("tool"));
        assert_eq!(from_legacy("Gesundheit"), Legacy::Mapped("body"));
    }

    /// The ordering trap: every e-bike spelling contains a bike spelling. Matching is exact
    /// rather than substring precisely so this cannot go wrong, and this test pins it.
    #[test]
    fn e_bike_does_not_become_a_bike() {
        assert_eq!(from_legacy("e-bike"), Legacy::Mapped("e_bike"));
        assert_eq!(from_legacy("ebike"), Legacy::Mapped("e_bike"));
        assert_eq!(from_legacy("E-Bike"), Legacy::Mapped("e_bike"));
        assert_eq!(from_legacy("pedelec"), Legacy::Mapped("e_bike"));
    }

    #[test]
    fn unknown_text_is_not_guessed_at() {
        assert_eq!(from_legacy("Gravelbike Custom"), Legacy::Unmapped);
        assert_eq!(from_legacy("Rennrad"), Legacy::Unmapped);
        assert_eq!(from_legacy(""), Legacy::Unmapped);
        assert_eq!(from_legacy("   "), Legacy::Unmapped);
    }

    #[test]
    fn every_mapped_target_is_a_real_type() {
        for word in LEGACY.iter() {
            assert!(is_valid(word.1), "{} maps to unknown type {}", word.0, word.1);
        }
        assert!(is_valid("other"));
        assert!(!is_valid("vehicle"));
    }
}
