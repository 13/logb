import type { ObjectInput } from './types';
export interface SavedObjectTemplate { id: string; name: string; input: ObjectInput }
const KEY = 'logb.object-templates';
export function loadObjectTemplates(): SavedObjectTemplate[] {
  try { const value = JSON.parse(localStorage.getItem(KEY) ?? '[]'); return Array.isArray(value) ? value : []; } catch { return []; }
}
export function saveObjectTemplate(input: ObjectInput): SavedObjectTemplate[] {
  const list = loadObjectTemplates();
  const template = { id: crypto.randomUUID?.() ?? `${Date.now()}`, name: input.name.trim(), input: structuredClone(input) };
  const next = [...list.filter((t) => t.name.toLowerCase() !== template.name.toLowerCase()), template];
  localStorage.setItem(KEY, JSON.stringify(next)); return next;
}
export function removeObjectTemplate(id: string): SavedObjectTemplate[] {
  const next = loadObjectTemplates().filter((t) => t.id !== id); localStorage.setItem(KEY, JSON.stringify(next)); return next;
}
