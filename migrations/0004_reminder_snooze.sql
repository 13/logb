-- A snooze hides a reminder until a date without destroying why it was due, which rewriting
-- due_date used to do -- and rewriting could not suppress a counter-based reminder at all.
ALTER TABLE reminders ADD COLUMN snoozed_until TEXT;
