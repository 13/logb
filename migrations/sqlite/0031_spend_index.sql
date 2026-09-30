-- `/stats` sums every costed entry by object, month and category (`api::stats::read`), and the
-- year picker and the purchase check ask the same rows again. Through `idx_activities_object_date`
-- each of those rows was a lookup into the table itself; this index holds exactly the columns
-- those statements read, for exactly the rows they want, so they are answered from the index
-- alone. `deleted_at` is always NULL in it and is listed only so SQLite counts the index as
-- covering: its planner does not treat a column named only in the index's WHERE as available.
-- Entries without a cost -- most readings, most check-ups -- are not in it at all.
CREATE INDEX idx_activities_spend ON activities(object_id, date, category, cost_cents, deleted_at)
  WHERE deleted_at IS NULL AND cost_cents IS NOT NULL;
