import { messageText, type Translate } from './api-error';
import type { OpKind, QueuedOp } from './outbox';

/** What each queued write was, in words -- the op kind and API path mean nothing to a reader. */
const KIND_KEYS: Record<OpKind, string> = {
  'object.create': 'outbox.kind-object-create',
  'activity.create': 'outbox.kind-activity-create',
  'activity.update': 'outbox.kind-activity-update',
  'attachment.upload': 'outbox.kind-attachment-upload',
  'reminder.create': 'outbox.kind-reminder-create',
  'reminder.done': 'outbox.kind-reminder-done',
};

/** A dead op keeps only the failure's MESSAGE (`lastError`, see `replay` in ./outbox.ts), not its
 *  code -- so the server's fixed sentences (src/error.rs) are recognised by their text here. */
const SERVER_SENTENCES: Record<string, string> = {
  'authentication required': 'error.unauthorized',
  forbidden: 'error.forbidden',
  'not found': 'error.not-found',
  'payload too large': 'error.too-large',
  'too many requests': 'error.too-many-requests',
  'internal error': 'error.internal',
  'the database is busy, please retry': 'error.busy',
};

export interface FailedWrite {
  /** "New entry", "Photo or file", … */
  what: string;
  /** The entry's title, the object's name or the file's name, when there is one. */
  name: string | null;
  /** Why it failed, translated where the reason is one we know. */
  reason: string | null;
}

function text(v: unknown): string | null {
  return typeof v === 'string' && v.trim() ? v.trim() : null;
}

export function describeFailedWrite(op: QueuedOp, t: Translate): FailedWrite {
  const what = t(KIND_KEYS[op.kind] ?? 'outbox.kind-other');
  const name = text(op.body?.title) ?? text(op.body?.name) ?? text(op.filename);
  let reason: string | null = null;
  if (op.lastError) {
    const known = SERVER_SENTENCES[op.lastError];
    reason = known ? t(known) : messageText(op.lastError, t);
  }
  return { what, name, reason };
}
