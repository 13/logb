<script lang="ts">
  import { tagColorIndex } from './tags';
  let { tags, onselect, active = null }: { tags: string[]; onselect?: (tag: string) => void; active?: string | null } = $props();
</script>

{#if tags.length > 0}
  <span class="tags">
    {#each tags as tag (tag)}
      {#if onselect}
        <!-- stopPropagation: a chip inside a clickable card must filter, not open the card. -->
        <button type="button" class={`tag tag-${tagColorIndex(tag)}`} class:active={active === tag} aria-pressed={active === tag}
                onclick={(e) => { e.stopPropagation(); onselect(tag); }}>{tag}</button>
      {:else}
        <span class={`tag tag-${tagColorIndex(tag)}`}>{tag}</span>
      {/if}
    {/each}
  </span>
{/if}
