use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The `by_category` bucket for purchase prices. Not an activity category: an object's
/// `purchase_price_cents` is a field of the object, and it gets its own row so it is never
/// mistaken for an activity logged under `purchase`.
pub const PURCHASE_PRICE: &str = "purchase_price";

/// One of the user's non-deleted objects, as the statistics need it.
#[derive(Clone, Debug)]
pub struct ObjectRow {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    /// The object's `type` column.
    pub kind: String,
    pub archived: bool,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    /// RFC 3339, as `db::now` writes it.
    pub created_at: String,
}

/// Money spent on one object in one month under one category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spend {
    pub object_id: i64,
    /// `YYYY-MM`.
    pub month: String,
    pub category: String,
    pub cost_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Amount {
    pub bucket: String,
    pub cost_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObjectNode {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub archived: bool,
    /// This object's own spend plus every descendant's.
    pub cost_cents: i64,
    pub children: Vec<ObjectNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub total_cents: i64,
    pub years: Vec<String>,
    pub over_time: Vec<Amount>,
    pub by_object: Vec<ObjectNode>,
    pub by_type: Vec<Amount>,
    pub by_category: Vec<Amount>,
}

/// Purchase prices as spend, one entry per object that has a positive price.
///
/// Dated by `purchase_date`, or by the day the object was created when no purchase date was
/// entered -- the price was paid at some point, and the moment it was recorded is the best date
/// there is. An object in `purchased` already has a costed `purchase` activity: that entry is
/// the purchase, and adding the price as well would count the same money twice.
pub fn purchase_spend(objects: &[ObjectRow], purchased: &HashSet<i64>) -> Vec<Spend> {
    objects
        .iter()
        .filter(|o| !purchased.contains(&o.id))
        .filter_map(|o| {
            let cents = o.purchase_price_cents.filter(|c| *c > 0)?;
            let date = o.purchase_date.as_deref().unwrap_or(&o.created_at);
            Some(Spend { object_id: o.id, month: date.get(..7)?.to_string(), category: PURCHASE_PRICE.into(), cost_cents: cents })
        })
        .collect()
}

/// The whole statistics response for `spend`, restricted to `year` when one is given.
///
/// `years` is computed before the filter, so the year picker always offers every year that has
/// spend, whichever one is selected.
pub fn summarize(objects: &[ObjectRow], spend: &[Spend], year: Option<i32>) -> Stats {
    let mut years: Vec<String> = spend
        .iter()
        .filter(|s| s.cost_cents > 0)
        .filter_map(|s| s.month.get(..4).map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    years.reverse();

    let prefix = year.map(|y| format!("{y:04}-"));
    let selected: Vec<&Spend> = spend
        .iter()
        .filter(|s| s.cost_cents > 0)
        .filter(|s| prefix.as_deref().is_none_or(|p| s.month.starts_with(p)))
        .collect();

    let over_time = match year {
        Some(y) => (1..=12)
            .map(|m| {
                let bucket = format!("{y:04}-{m:02}");
                let cost_cents = selected.iter().filter(|s| s.month == bucket).map(|s| s.cost_cents).sum();
                Amount { bucket, cost_cents }
            })
            .collect(),
        None => {
            let mut per_year: BTreeMap<String, i64> = BTreeMap::new();
            for s in &selected {
                if let Some(y) = s.month.get(..4) {
                    *per_year.entry(y.to_string()).or_default() += s.cost_cents;
                }
            }
            per_year.into_iter().map(|(bucket, cost_cents)| Amount { bucket, cost_cents }).collect()
        }
    };

    let mut own: HashMap<i64, i64> = HashMap::new();
    let mut by_category: HashMap<String, i64> = HashMap::new();
    for s in &selected {
        *own.entry(s.object_id).or_default() += s.cost_cents;
        *by_category.entry(s.category.clone()).or_default() += s.cost_cents;
    }

    let mut by_type: HashMap<String, i64> = HashMap::new();
    for o in objects {
        if let Some(c) = own.get(&o.id) {
            *by_type.entry(o.kind.clone()).or_default() += c;
        }
    }

    Stats {
        total_cents: selected.iter().map(|s| s.cost_cents).sum(),
        years,
        over_time,
        by_object: roll_up(objects, &own),
        by_type: sorted(by_type),
        by_category: sorted(by_category),
    }
}

/// Largest first, then by name so equal amounts do not shuffle between requests.
fn sorted(map: HashMap<String, i64>) -> Vec<Amount> {
    let mut list: Vec<Amount> = map
        .into_iter()
        .filter(|(_, c)| *c > 0)
        .map(|(bucket, cost_cents)| Amount { bucket, cost_cents })
        .collect();
    list.sort_by(|a, b| b.cost_cents.cmp(&a.cost_cents).then_with(|| a.bucket.cmp(&b.bucket)));
    list
}

/// The object tree with each node carrying its subtree's spend.
///
/// An object whose parent is not among `objects` is a root: the list holds only non-deleted
/// objects, and a child must not vanish from the totals because of what happened to its parent.
/// Subtrees that spent nothing are dropped. The hierarchy API refuses cycles, so every object is
/// reached from exactly one root.
fn roll_up(objects: &[ObjectRow], own: &HashMap<i64, i64>) -> Vec<ObjectNode> {
    let ids: HashSet<i64> = objects.iter().map(|o| o.id).collect();
    let mut children: HashMap<Option<i64>, Vec<&ObjectRow>> = HashMap::new();
    for o in objects {
        let parent = o.parent_id.filter(|p| ids.contains(p));
        children.entry(parent).or_default().push(o);
    }

    fn build(parent: Option<i64>, children: &HashMap<Option<i64>, Vec<&ObjectRow>>, own: &HashMap<i64, i64>) -> Vec<ObjectNode> {
        let mut nodes: Vec<ObjectNode> = children
            .get(&parent)
            .into_iter()
            .flatten()
            .map(|o| {
                let kids = build(Some(o.id), children, own);
                let cost_cents = own.get(&o.id).copied().unwrap_or(0) + kids.iter().map(|k| k.cost_cents).sum::<i64>();
                ObjectNode { id: o.id, name: o.name.clone(), kind: o.kind.clone(), archived: o.archived, cost_cents, children: kids }
            })
            .filter(|n| n.cost_cents > 0)
            .collect();
        nodes.sort_by(|a, b| b.cost_cents.cmp(&a.cost_cents).then_with(|| a.name.cmp(&b.name)));
        nodes
    }

    build(None, &children, own)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(id: i64, parent_id: Option<i64>, name: &str, kind: &str) -> ObjectRow {
        ObjectRow {
            id, parent_id, name: name.into(), kind: kind.into(), archived: false,
            purchase_date: None, purchase_price_cents: None, created_at: "2024-01-01T10:00:00Z".into(),
        }
    }

    fn spend(object_id: i64, month: &str, category: &str, cost_cents: i64) -> Spend {
        Spend { object_id, month: month.into(), category: category.into(), cost_cents }
    }

    fn amounts(list: &[Amount]) -> Vec<(&str, i64)> {
        list.iter().map(|a| (a.bucket.as_str(), a.cost_cents)).collect()
    }

    /// House (1) > Garage (2) > Bulb (3), plus a car (4).
    fn tree() -> Vec<ObjectRow> {
        vec![obj(1, None, "House", "home"), obj(2, Some(1), "Garage", "other"),
             obj(3, Some(2), "Bulb", "appliance"), obj(4, None, "Car", "car")]
    }

    #[test]
    fn a_parent_carries_every_descendants_spend() {
        let rows = [spend(1, "2026-01", "repair", 1_000), spend(2, "2026-02", "maintenance", 200),
                    spend(3, "2026-03", "repair", 30), spend(4, "2026-01", "fuel", 500)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(s.total_cents, 1_730);
        assert_eq!(s.by_object.len(), 2, "two roots");
        let house = &s.by_object[0];
        assert_eq!((house.name.as_str(), house.cost_cents), ("House", 1_230), "largest first");
        assert_eq!((house.children[0].name.as_str(), house.children[0].cost_cents), ("Garage", 230));
        assert_eq!((house.children[0].children[0].name.as_str(), house.children[0].children[0].cost_cents), ("Bulb", 30));
        assert_eq!((s.by_object[1].name.as_str(), s.by_object[1].cost_cents), ("Car", 500));
    }

    #[test]
    fn a_subtree_that_spent_nothing_is_left_out() {
        let rows = [spend(4, "2026-01", "fuel", 500)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(s.by_object.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Car"]);
    }

    #[test]
    fn an_object_whose_parent_is_not_in_the_list_is_a_root() {
        let objects = vec![obj(2, Some(99), "Garage", "other")];
        let s = summarize(&objects, &[spend(2, "2026-01", "repair", 70)], None);
        assert_eq!(s.by_object[0].name, "Garage");
    }

    #[test]
    fn type_is_each_objects_own_not_its_roots() {
        let rows = [spend(1, "2026-01", "repair", 1_000), spend(3, "2026-01", "repair", 30)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(amounts(&s.by_type), [("home", 1_000), ("appliance", 30)]);
    }

    #[test]
    fn all_years_draws_one_bar_per_year_oldest_first_and_lists_years_newest_first() {
        let rows = [spend(4, "2024-05", "fuel", 10), spend(4, "2026-01", "fuel", 20), spend(4, "2026-09", "fuel", 5)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(amounts(&s.over_time), [("2024", 10), ("2026", 25)], "no bar for a year with no spend");
        assert_eq!(s.years, ["2026", "2024"]);
    }

    #[test]
    fn one_year_draws_all_twelve_months_and_filters_every_block() {
        let rows = [spend(4, "2025-12", "fuel", 999), spend(4, "2026-01", "fuel", 20), spend(1, "2026-03", "repair", 7)];
        let s = summarize(&tree(), &rows, Some(2026));
        assert_eq!(s.over_time.len(), 12);
        assert_eq!(s.over_time[0], Amount { bucket: "2026-01".into(), cost_cents: 20 });
        assert_eq!(s.over_time[1], Amount { bucket: "2026-02".into(), cost_cents: 0 }, "known zero, not missing");
        assert_eq!(s.over_time[11].bucket, "2026-12");
        assert_eq!(s.total_cents, 27);
        assert_eq!(amounts(&s.by_category), [("fuel", 20), ("repair", 7)]);
        assert_eq!(s.years, ["2026", "2025"], "the year list ignores the filter");
    }

    #[test]
    fn categories_are_sorted_largest_first_and_zero_rows_dropped() {
        let rows = [spend(4, "2026-01", "fuel", 5), spend(4, "2026-01", "repair", 50), spend(4, "2026-01", "other", 0)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(amounts(&s.by_category), [("repair", 50), ("fuel", 5)]);
    }

    #[test]
    fn archived_is_carried_through() {
        let mut objects = tree();
        objects[3].archived = true;
        let s = summarize(&objects, &[spend(4, "2026-01", "fuel", 5)], None);
        assert!(s.by_object[0].archived);
    }

    #[test]
    fn nothing_spent_is_an_empty_but_complete_answer() {
        let s = summarize(&tree(), &[], Some(2026));
        assert_eq!(s.total_cents, 0);
        assert!(s.years.is_empty() && s.by_object.is_empty() && s.by_type.is_empty() && s.by_category.is_empty());
        assert_eq!(s.over_time.len(), 12);
    }

    #[test]
    fn a_purchase_price_is_dated_by_its_purchase_date() {
        let mut house = obj(1, None, "House", "home");
        house.purchase_date = Some("2019-06-30".into());
        house.purchase_price_cents = Some(30_000_000);
        assert_eq!(purchase_spend(&[house], &HashSet::new()), [spend(1, "2019-06", PURCHASE_PRICE, 30_000_000)]);
    }

    #[test]
    fn a_purchase_price_without_a_date_falls_back_to_when_the_object_was_created() {
        let mut car = obj(4, None, "Car", "car");
        car.purchase_price_cents = Some(1_500_000);
        car.created_at = "2023-11-02T08:00:00Z".into();
        assert_eq!(purchase_spend(&[car], &HashSet::new()), [spend(4, "2023-11", PURCHASE_PRICE, 1_500_000)]);
    }

    #[test]
    fn a_purchase_price_is_skipped_when_a_costed_purchase_entry_already_records_it() {
        let mut car = obj(4, None, "Car", "car");
        car.purchase_price_cents = Some(1_500_000);
        assert!(purchase_spend(&[car], &HashSet::from([4])).is_empty());
    }

    #[test]
    fn no_price_or_a_zero_price_adds_nothing() {
        let mut free = obj(5, None, "Gift", "tool");
        free.purchase_price_cents = Some(0);
        assert!(purchase_spend(&[obj(4, None, "Car", "car"), free], &HashSet::new()).is_empty());
    }
}
