import type { ObjectInput } from './types';

/**
 * Object templates the user saved from the new-object form, kept on this device.
 *
 * Keyed per user: localStorage is one per origin, shared by every account that signs in on the
 * device, and one person's templates are not the next person's. `endSession` clears them all
 * (`forgetObjectTemplates`), like every other thing a session leaves behind.
 *
 * A template is what an object IS, not where it sits or who may see it: `parent_id`, `private`
 * and `archived` are reset, so a template saved from a private object filed in the garage does
 * not quietly file (or hide, or archive) every object made from it.
 */
export interface SavedObjectTemplate { id: string; name: string; input: ObjectInput }

const PREFIX = 'logb.object-templates';
const keyFor = (userId: number) => `${PREFIX}.${userId}`;

function strip(input: ObjectInput): ObjectInput {
  return { ...input, parent_id: null, private: false, archived: false };
}

export function loadObjectTemplates(userId: number | null): SavedObjectTemplate[] {
  if (userId === null) return [];
  try {
    const value = JSON.parse(localStorage.getItem(keyFor(userId)) ?? '[]');
    return Array.isArray(value) ? value.map((t: SavedObjectTemplate) => ({ ...t, input: strip(t.input) })) : [];
  } catch { return []; }
}

/** Writes `next` and returns it -- or, when the write fails (storage full or unavailable), the
 *  list as it still is, so the screen never shows a template that was not kept. */
function store(userId: number, next: SavedObjectTemplate[]): SavedObjectTemplate[] {
  try {
    localStorage.setItem(keyFor(userId), JSON.stringify(next));
    return next;
  } catch {
    return loadObjectTemplates(userId);
  }
}

export function saveObjectTemplate(userId: number | null, input: ObjectInput): SavedObjectTemplate[] {
  if (userId === null) return [];
  const name = input.name.trim();
  const template = { id: crypto.randomUUID?.() ?? `${Date.now()}`, name, input: strip(structuredClone(input)) };
  const list = loadObjectTemplates(userId);
  return store(userId, [...list.filter((t) => t.name.toLowerCase() !== name.toLowerCase()), template]);
}

export function removeObjectTemplate(userId: number | null, id: string): SavedObjectTemplate[] {
  if (userId === null) return [];
  return store(userId, loadObjectTemplates(userId).filter((t) => t.id !== id));
}

/** Every user's templates on this device, and the one shared list older versions kept. */
export function forgetObjectTemplates(): void {
  try {
    const keys: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k === PREFIX || k?.startsWith(`${PREFIX}.`)) keys.push(k);
    }
    for (const k of keys) localStorage.removeItem(k);
  } catch { /* nothing to clear */ }
}
