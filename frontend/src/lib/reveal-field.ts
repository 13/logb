import { tick } from 'svelte';

/**
 * Brings a refused field into view and focus after a failed save. A field inside a closed "More
 * details" is shown first, through that section's own toggle, so the focus has somewhere to land.
 * Answers false when the form has no such element (the field is not shown for this entry); the
 * caller then puts the message on the form's own line.
 */
export async function revealField(id: string): Promise<boolean> {
  await tick();
  const el = document.getElementById(id);
  if (!el) return false;
  const region = el.closest<HTMLElement>('[hidden]');
  if (region?.id) document.querySelector<HTMLButtonElement>(`[aria-controls="${region.id}"]`)?.click();
  await tick();
  el.focus();
  return true;
}
