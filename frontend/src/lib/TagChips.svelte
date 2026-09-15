<script lang="ts">
  import { t } from '../i18n';
  import { foldTag, tagColorIndex } from './tags';
  let { tags, onselect, active = null, navigates = false }:
    {
      tags: string[]; onselect?: (tag: string) => void; active?: string | null;
      /** The chip goes somewhere else (a search hit) rather than toggling a filter on this screen,
       *  so it is no toggle: no `aria-pressed`, and a label saying where it leads. */
      navigates?: boolean;
    } = $props();
  // By folded name, like the filter itself: "winter" is the tag a "Winter" filter selects.
  const isActive = (tag: string) => active !== null && foldTag(active) === foldTag(tag);
</script>

{#if tags.length > 0}
  <span class="tags">
    {#each tags as tag (tag)}
      {#if onselect && navigates}
        <button type="button" class={`tag tag-${tagColorIndex(tag)}`} aria-label={$t('tags.show', { tag })}
                onclick={(e) => { e.stopPropagation(); onselect(tag); }}>{tag}</button>
      {:else if onselect}
        <!-- stopPropagation: a chip inside a clickable card must filter, not open the card. -->
        <button type="button" class={`tag tag-${tagColorIndex(tag)}`} class:active={isActive(tag)} aria-pressed={isActive(tag)}
                onclick={(e) => { e.stopPropagation(); onselect(tag); }}>{tag}</button>
      {:else}
        <span class={`tag tag-${tagColorIndex(tag)}`}>{tag}</span>
      {/if}
    {/each}
  </span>
{/if}
