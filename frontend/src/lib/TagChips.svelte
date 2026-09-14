<script lang="ts">
  import { foldTag, tagColorIndex } from './tags';
  let { tags, onselect, active = null }: { tags: string[]; onselect?: (tag: string) => void; active?: string | null } = $props();
  // By folded name, like the filter itself: "winter" is the tag a "Winter" filter selects.
  const isActive = (tag: string) => active !== null && foldTag(active) === foldTag(tag);
</script>

{#if tags.length > 0}
  <span class="tags">
    {#each tags as tag (tag)}
      {#if onselect}
        <!-- stopPropagation: a chip inside a clickable card must filter, not open the card. -->
        <button type="button" class={`tag tag-${tagColorIndex(tag)}`} class:active={isActive(tag)} aria-pressed={isActive(tag)}
                onclick={(e) => { e.stopPropagation(); onselect(tag); }}>{tag}</button>
      {:else}
        <span class={`tag tag-${tagColorIndex(tag)}`}>{tag}</span>
      {/if}
    {/each}
  </span>
{/if}
