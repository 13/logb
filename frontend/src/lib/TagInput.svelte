<script lang="ts">
  import { t } from '../i18n';
  import { errorClass, labelClass } from '$lib/components/ui/field/classes.js';
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

  let field: HTMLInputElement;
  // Enter, a comma and blur all turn typed text into a tag, but a form can be submitted without
  // any of them reaching this field: an Android keyboard's Enter often arrives as key
  // "Unidentified" (so `onkeydown` lets it through) and submits with the focus still here. The
  // typed tag was then silently left out of the save. So the typed text is added on the form's
  // submit too -- in the capture phase, which runs before the form's own `onsubmit` reads
  // `tags` -- and a tag that cannot be added stops the submit, keeping its error on screen
  // instead of saving without it.
  $effect(() => {
    const form = field.form;
    if (!form) return;
    const commit = (e: SubmitEvent) => {
      if (text.trim() === '' || add(text)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      // The submit may have come from far down the form: bring the field and its error into view.
      field.focus();
    };
    form.addEventListener('submit', commit, { capture: true });
    return () => form.removeEventListener('submit', commit, { capture: true });
  });
</script>

<div data-slot="tag-field" class="flex min-w-0 flex-col gap-1.5">
  <label for={id} class={labelClass}>{label}</label>
  <!-- Looks like one text field: the chips sit inside the box, and the box shows the focus. -->
  <div data-testid="tag-input" data-invalid={error ? '' : undefined}
       class="data-invalid:border-destructive flex min-h-12 flex-wrap items-center gap-1 rounded-lg border border-input bg-card px-2 py-1.5 focus-within:outline-2 focus-within:outline-solid focus-within:outline-offset-2 focus-within:outline-ring">
    {#each tags as tag (tag)}
      <span class={`tag tag-${tagColorIndex(tag)}`}>{tag}
        <!-- A 32 px tall hit area around the bare glyph: it reaches left over the chip's own text
             and right to half the gap, so it never covers the next chip. -->
        <button type="button" data-slot="tag-remove" aria-label={$t('tags.remove', { tag })} onclick={() => (tags = removeTag(tags, tag))}
                class="relative cursor-pointer border-0 bg-transparent p-0 pl-0.5 text-inherit before:absolute before:top-[calc(50%-16px)] before:bottom-[calc(50%-16px)] before:-left-3.5 before:-right-2.5 before:content-[''] focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-1 focus-visible:outline-ring">×</button>
      </span>
    {/each}
    <input {id} data-slot="tag-text" bind:this={field} bind:value={text} {onkeydown} {oninput} onblur={() => text.trim() && add(text)}
           placeholder={$t('tags.placeholder')} autocomplete="off" list={`${id}-list`}
           aria-describedby={error ? `${id}-error` : undefined} aria-invalid={error ? true : undefined}
           class="h-9 min-w-32 flex-1 scroll-my-24 border-0 bg-transparent px-1 text-base text-foreground placeholder:text-muted-foreground focus-visible:outline-none" />
    <datalist id={`${id}-list`}>{#each offered as s (s)}<option value={s}></option>{/each}</datalist>
  </div>
  <!-- Always rendered, so the live region exists before the first error is announced. -->
  <p class={errorClass} id={`${id}-error`} aria-live="polite" hidden={!error}>{error}</p>
</div>
