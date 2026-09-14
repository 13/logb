<script lang="ts">
  import { t } from '../i18n';
  import { addTag, removeTag, splitTyped, suggestTags, tagColorIndex } from './tags';
  import type { TagCount } from './types';

  let { tags = $bindable([]), suggestions, label, id }: { tags: string[]; suggestions: TagCount[]; label: string; id: string } = $props();
  let text = $state('');
  let error = $state('');
  const offered = $derived(suggestTags(suggestions, tags, text));

  /** Adds `raw` as a tag. On failure the typed text stays, so it can be shortened rather than retyped. */
  function add(raw: string): boolean {
    const r = addTag(tags, raw);
    if ('error' in r) { if (r.error !== 'empty') error = $t(`tags.${r.error}`); return false; }
    tags = r.tags; text = ''; error = '';
    return true;
  }
  function onkeydown(e: KeyboardEvent) {
    // Enter on an empty field is left alone, so it still submits the form.
    if (e.key === 'Enter' && text.trim() === '') return;
    if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); add(text); }
    else if (e.key === 'Backspace' && text === '' && tags.length > 0) { tags = tags.slice(0, -1); }
  }
  // Android keyboards often report `e.key` as "Unidentified", so a typed comma only shows up here.
  // `splitTyped` turns every segment before the last comma into a tag via `addTag`; what follows
  // stays as the text, a failed segment included, with its error translated for display.
  function oninput(e: Event) {
    // The element's own value: this must not depend on whether `bind:value` has run yet.
    const value = (e.currentTarget as HTMLInputElement).value;
    const r = splitTyped(tags, value);
    tags = r.tags; text = r.text; error = r.error ? $t(`tags.${r.error}`) : '';
  }
</script>

<div class="field">
  <label for={id}>{label}</label>
  <div class="tag-input">
    {#each tags as tag (tag)}
      <span class={`tag tag-${tagColorIndex(tag)}`}>{tag}
        <button type="button" class="remove" aria-label={$t('tags.remove', { tag })} onclick={() => (tags = removeTag(tags, tag))}>×</button>
      </span>
    {/each}
    <input {id} bind:value={text} {onkeydown} {oninput} onblur={() => text.trim() && add(text)} placeholder={$t('tags.placeholder')} autocomplete="off" list={`${id}-list`}
           aria-describedby={error ? `${id}-error` : undefined} />
    <datalist id={`${id}-list`}>{#each offered as s (s)}<option value={s}></option>{/each}</datalist>
  </div>
  <!-- Always rendered, so the live region exists before the first error is announced. -->
  <p class="error" id={`${id}-error`} aria-live="polite" hidden={!error}>{error}</p>
</div>

<style>
  .tag-input { display: flex; flex-wrap: wrap; gap: var(--space-1); align-items: center; }
  .tag-input input { flex: 1 1 8rem; min-width: 8rem; }
  .remove { position: relative; background: none; border: 0; padding: 0 0 0 2px; min-height: 0; color: inherit; font: inherit; cursor: pointer; }
  /* A 32px-tall hit area around the bare glyph -- short of WCAG 2.2 AA's 24x24 minimum target
     size horizontally, since it must stay inside the chip's own row, but well past it vertically.
     It reaches left over the chip's own (inert) text and right only to half the gap past the
     chip's padding, so it never covers the next chip. */
  .remove::before {
    content: ''; position: absolute; top: calc(50% - 16px); bottom: calc(50% - 16px);
    left: -14px; right: calc(-1 * var(--space-2) - var(--space-1) / 2);
  }
</style>
