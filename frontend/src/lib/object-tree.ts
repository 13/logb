import type { MemObject } from './types';

/** Every object in `all` that may legally become `selfId`'s parent: not the object itself, and
 *  not anything reachable by following `parent_id` down from it -- which the server would
 *  refuse anyway as a cycle, but offering it at all would be a choice that is always wrong.
 *  `selfId` is `null` when creating a brand-new object, which cannot yet be anyone's ancestor. */
export function excludingDescendants(all: MemObject[], selfId: number | null): MemObject[] {
  if (selfId === null) return all;
  const childrenOf = new Map<number, number[]>();
  for (const o of all) {
    if (o.parent_id === null) continue;
    childrenOf.set(o.parent_id, [...(childrenOf.get(o.parent_id) ?? []), o.id]);
  }
  const excluded = new Set<number>([selfId]);
  const stack = [selfId];
  while (stack.length > 0) {
    const id = stack.pop()!;
    for (const child of childrenOf.get(id) ?? []) {
      if (!excluded.has(child)) { excluded.add(child); stack.push(child); }
    }
  }
  return all.filter((o) => !excluded.has(o.id));
}
