-- The retention purge (`sync::feed::purge`) looks for tombstones with `deleted_at IS NOT NULL
-- AND deleted_at < $1` on five tables every hour. Without an index that is a scan of every live
-- row as well; these partial indexes hold only the tombstones, so they stay small and cost the
-- ordinary write path nothing until a row is deleted.
CREATE INDEX idx_objects_deleted_at ON objects(deleted_at) WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_activities_deleted_at ON activities(deleted_at) WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_reminders_deleted_at ON reminders(deleted_at) WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_attachments_deleted_at ON attachments(deleted_at) WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_object_types_deleted_at ON object_types(deleted_at) WHERE deleted_at IS NOT NULL;

-- `reminders.done_activity_id` is `ON DELETE SET NULL`, so every activity the purge removes
-- scanned `reminders` for rows naming it; the sync cascade asks the same question by uuid.
CREATE INDEX idx_reminders_done_activity ON reminders(done_activity_id);
