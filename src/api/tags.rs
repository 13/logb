use crate::auth::AuthUser;
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use std::collections::HashMap;

pub fn router() -> Router<App> {
    Router::new().route("/tags", get(list))
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

/// Every tag the caller uses, for suggestions and filter chips: across non-deleted objects and
/// the non-deleted entries of non-deleted objects, archived ones included -- an archived car's
/// "Winter" is still a word the user chose.
///
/// The database groups identical `tags` texts, so each distinct array is parsed and folded once
/// rather than once per row: most entries carry `[]` or one of a handful of combinations, and
/// twenty thousand rows come back as a few dozen.
async fn list(user: AuthUser, State(state): State<App>) -> Result<Json<Vec<TagCount>>, AppError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT tags, count(*) FROM ( \
           SELECT tags FROM objects WHERE user_id = $1 AND deleted_at IS NULL \
           UNION ALL \
           SELECT a.tags FROM activities a JOIN objects o ON o.id = a.object_id \
           WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
         ) t GROUP BY tags",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(count(rows.iter().map(|(t, n)| (t.as_str(), *n)))))
}

/// Counts tags by their folded form, from `(tags text, rows carrying it)` pairs. Each row's
/// tags are already distinct under `fold`, so the count is the number of rows carrying the
/// tag. The spelling shown is the one most rows use (ties: alphabetical), so "Winter" twice
/// and "winter" once reads "Winter".
fn count<'a>(rows: impl Iterator<Item = (&'a str, i64)>) -> Vec<TagCount> {
    let mut by_fold: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for (text, rows) in rows {
        for tag in tags::from_json(text) {
            *by_fold.entry(tags::fold(&tag)).or_default().entry(tag).or_default() += rows;
        }
    }
    let mut out: Vec<TagCount> = by_fold
        .into_values()
        .map(|spellings| {
            let count = spellings.values().sum();
            let (tag, _) = spellings
                .into_iter()
                .max_by(|(a, na), (b, nb)| na.cmp(nb).then_with(|| b.cmp(a)))
                .expect("a folded tag has at least one spelling");
            TagCount { tag, count }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_used_spelling_wins_and_ties_go_alphabetical() {
        let rows = [(r#"["winter","Lease"]"#, 1), (r#"["Winter"]"#, 2), (r#"["b"]"#, 1), (r#"["B"]"#, 1), ("[]", 7)];
        assert_eq!(
            count(rows.into_iter()),
            vec![
                TagCount { tag: "Winter".into(), count: 3 },
                TagCount { tag: "B".into(), count: 2 },
                TagCount { tag: "Lease".into(), count: 1 },
            ]
        );
    }
}
