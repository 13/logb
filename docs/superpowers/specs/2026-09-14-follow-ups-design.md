# Follow-ups after statistics

Status: approved, not implemented. Clears the leftovers recorded while building
`2026-09-14-statistics-design.md` and `2026-09-14-object-cost-depth-design.md`.

1. **Flaky settings spec.** `14-settings.spec.ts` "a row carries its current value" counts the
   People page's Remove buttons before the user list has loaded, sees none, skips its cleanup and
   then reads "19 users". It waits for the signed-in admin's row before counting. Test-only change.
2. **Object page rewrites its own address.** `ObjectDetail.svelte` always writes `?tab=timeline`, so
   `/objects/5` becomes `/objects/5?tab=timeline` a moment after it opens. The default tab is left
   out of the URL; any other tab is written; the address is only replaced when it actually changes.
   Links such as `?tab=reminders` keep working. `20-statistics.spec.ts` drops the `Promise.all`
   workaround it needed for this.
3. **Statistics year in the URL.** `Stats.svelte` reads `?year=` on load (accepted only as four
   digits; anything else means all years) and keeps the address in step as the year changes, as
   Search keeps `?q=`. A reload or the back button from an object returns to the same year.
4. **Include-contents switch for archived-only children.** The Info tab also fetches the object's
   archived children and offers the switch when it has any child at all, archived or not. The
   Contents list is unchanged (active children only). The cost depth spec's block 1 is reworded
   back to "at least one non-deleted child".
5. **One checkbox rule.** `.row.toggle input { flex: none; width: 20px; height: 20px; }` moves into
   `app.css` next to `.row > * { flex: 1; }`, replacing the five local copies (ObjectForm,
   ReminderForm, settings/People, Stats, Insights).

## Tests

- Item 1: the spec itself, run alone and inside the full suite.
- Item 2: existing object specs (02, 12, 21) and the simplified 20.
- Item 3: 20-statistics selects 2026, sees `year=2026` in the URL, reloads, and still reads the
  2026 total with the picker on 2026.
- Item 4: 21-object-cost-depth adds a house whose only child is archived: the switch is shown and
  turning it on adds the child's cost to the ownership total.
- Item 5: 11-controls, 14-settings, 20, 21 and a 390px screenshot of a form with a toggle.

## Out of scope

Rewriting old commit trailers.
