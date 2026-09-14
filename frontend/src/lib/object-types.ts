import { get } from 'svelte/store';
import { OBJECT_TYPES, type Category, type ObjectType } from './types';
import type { IconName } from './Icon.svelte';
import * as registry from './type-registry';

// The old entry points, kept as thin wrappers over the registry so existing call sites keep
// working while they move to `type-registry.ts`, which also knows the user's own types.
export { OBJECT_TYPES };
export function typeIcon(t: ObjectType): IconName { return registry.typeIcon(t, get(registry.customTypes)); }
export function categoriesFor(t: ObjectType, current?: Category): Category[] {
  return registry.categoriesFor(t, get(registry.customTypes), current);
}
