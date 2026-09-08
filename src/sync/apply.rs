//! Applying an incoming op, and the rule that decides whether it may.

/// Whether an incoming edit supersedes the stored one for a field.
///
/// Timestamps are RFC3339 UTC with a fixed number of digits, so a lexical comparison is also
/// a chronological one and no parsing is needed. Equal timestamps break on `device_id`, which
/// is arbitrary but *consistent*: every device applying the same pair of ops reaches the same
/// answer without talking to any other device, which is what keeps two phones from converging
/// on different values. An op identical to the stored one does not win, so a replay is a
/// no-op rather than a rewrite.
pub fn wins(
    incoming_edited_at: &str,
    incoming_device: &str,
    stored_edited_at: &str,
    stored_device: &str,
) -> bool {
    match incoming_edited_at.cmp(stored_edited_at) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => incoming_device > stored_device,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_edit_wins() {
        assert!(wins("2026-01-02T00:00:00Z", "phone", "2026-01-01T00:00:00Z", "desktop"));
    }

    #[test]
    fn an_older_edit_loses() {
        assert!(!wins("2026-01-01T00:00:00Z", "phone", "2026-01-02T00:00:00Z", "desktop"));
    }

    #[test]
    fn a_tie_breaks_on_device_id_with_the_greater_winning() {
        let t = "2026-01-01T00:00:00Z";
        assert!(wins(t, "phone", t, "desktop"), "phone > desktop");
        assert!(!wins(t, "desktop", t, "phone"), "desktop < phone");
    }

    #[test]
    fn a_replay_of_the_same_op_does_not_win() {
        let t = "2026-01-01T00:00:00Z";
        assert!(!wins(t, "phone", t, "phone"), "identical edit is not newer than itself");
    }
}
