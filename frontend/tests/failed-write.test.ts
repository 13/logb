import { describe, expect, it } from 'vitest';
import { describeFailedWrite } from '../src/lib/failed-write';
import type { QueuedOp } from '../src/lib/outbox';
import en from '../src/i18n/en';

const t = (key: string, vars?: Record<string, string | number>) => {
  let s = (en as Record<string, string>)[key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
};

const op = (over: Partial<QueuedOp>): QueuedOp =>
  ({ id: 'x', kind: 'activity.create', path: '/objects/3/activities', body: {}, attempts: 3, dead: true, ...over });

describe('describeFailedWrite', () => {
  it('names every kind of write in words, never as its op kind or API path', () => {
    for (const kind of ['object.create', 'activity.create', 'activity.update', 'attachment.upload', 'reminder.create', 'reminder.done'] as const) {
      const d = describeFailedWrite(op({ kind }), t);
      expect(d.what, kind).not.toBe(kind);
      expect(d.what, kind).not.toMatch(/^[a-z]+\.[a-z]+$/);
      expect(Object.values(d).join(' '), kind).not.toContain('/objects');
    }
  });

  it('adds what the write was called: an entry or reminder title, an object name, a file name', () => {
    expect(describeFailedWrite(op({ body: { title: 'Oil change' } }), t).name).toBe('Oil change');
    expect(describeFailedWrite(op({ kind: 'object.create', body: { name: 'Golf' } }), t).name).toBe('Golf');
    expect(describeFailedWrite(op({ kind: 'attachment.upload', filename: 'receipt.jpg' }), t).name).toBe('receipt.jpg');
    expect(describeFailedWrite(op({ body: { title: '   ' } }), t).name).toBeNull();
  });

  it("says why in the reader's language where it can", () => {
    expect(describeFailedWrite(op({ lastError: 'not found' }), t).reason).toBe(en['error.not-found']);
    expect(describeFailedWrite(op({ lastError: 'payload too large' }), t).reason).toBe(en['error.too-large']);
    expect(describeFailedWrite(op({ lastError: 'Failed to fetch' }), t).reason).toBe(en['error.offline']);
    expect(describeFailedWrite(op({ lastError: 'outbox.queue-failed' }), t).reason).toBe(en['outbox.queue-failed']);
    expect(describeFailedWrite(op({ lastError: 'title must not be empty' }), t).reason).toBe('title must not be empty');
    expect(describeFailedWrite(op({}), t).reason).toBeNull();
  });
});
