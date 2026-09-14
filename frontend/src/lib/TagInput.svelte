<script lang="ts">
  import { t } from '../i18n';
  import { addTag, removeTag, suggestTags, tagColorIndex } from './tags';
  import type { TagCount } from './types';

  let { tags = $bindable([]), suggestions, label, id }: { tags: string[]; suggestions: TagCount[]; label: string; id: string } = $props();
  let text = $state('');
  let error = $state('');
  const offered = $derived(suggestTags(suggestions, tags, text));

  function add(raw: string) {
    const r = addTag(tags, raw);
    if ('error' in r) { if (r.error !== 'empty') error = $t(`tags.${r.error}`); return; }
    tags = r.tags; text = ''; error = '';
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); add(text); }
    else if (e.key === 'Backspace' && text === '' && tags.length > 0) { tags = tags.slice(0, -1); }
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
    <input {id} bind:value={text} {onkeydown} onblur={() => text.trim() && add(text)} placeholder={$t('tags.placeholder')} autocomplete="off" list={`${id}-list`} />
    <datalist id={`${id}-list`}>{#each offered as s (s)}<option value={s}></option>{/each}</datalist>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .tag-input { display: flex; flex-wrap: wrap; gap: var(--space-1); align-items: center; }
  .tag-input input { flex: 1 1 8rem; min-width: 8rem; }
  .remove { background: none; border: 0; padding: 0 0 0 2px; min-height: 0; color: inherit; font: inherit; cursor: pointer; }
</style>
