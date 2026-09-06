-- An id the client generates before it sends, so a create it retried after a lost response
-- resolves to the row it already made rather than a second copy of it. Partial indexes so the
-- column stays free for every row written by an online client, which sends nothing.
ALTER TABLE activities ADD COLUMN client_op_id TEXT;
CREATE UNIQUE INDEX idx_activities_client_op
  ON activities(client_op_id) WHERE client_op_id IS NOT NULL;

ALTER TABLE attachments ADD COLUMN client_op_id TEXT;
CREATE UNIQUE INDEX idx_attachments_client_op
  ON attachments(client_op_id) WHERE client_op_id IS NOT NULL;
