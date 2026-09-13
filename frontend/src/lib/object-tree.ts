import type { MemObject } from './types';

/** Every object in `all` that may legally become `selfId`'s parent: not the object itself, and
 *  not anything reachable by following `parent_id` down from it -- which the server would
 *  refuse anyway as a cycle, but offering it at all would be a choice that is always wrong.
 *  `selfId` is `null` when creating a brand-new object, which cannot yet be anyone's ancestor.
 *
 *  `all` must be the *complete* tree, not a filtered view of it. The walk can only follow links
 *  it can see, so an object handed a list with an intermediate missing -- an archived room, say
 *  -- will not find what hangs below that gap and will happily call a grandchild legal. The
 *  server's own check walks the real table and does see it, so the two disagree and the user
 *  gets a 400 on save. Filtering is the caller's job, and it belongs *after* this call: see
 *  `ObjectForm.svelte`, which merges both archived states before asking. */
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
