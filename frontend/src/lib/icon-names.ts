/** Every icon `Icon.svelte` draws. In a plain `.ts` file, not only in the component, so `types.ts`
 *  can name it without importing a `.svelte` file -- which the Node-side type check (tests that
 *  read source files) cannot resolve. `Icon.svelte` re-exports it, so existing imports still work. */
export type IconName = 'back' | 'settings' | 'search' | 'document' | 'camera' | 'edit' | 'repeat'
  | 'plus'
  | 'car' | 'e-bike' | 'bike' | 'motorcycle' | 'home' | 'appliance' | 'tool' | 'body' | 'object'
  | 'palette' | 'person' | 'key' | 'box' | 'people' | 'database' | 'chevron' | 'logout' | 'bell' | 'chart';
