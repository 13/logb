/** One way to add an entry to an object, as the "+ Log" action offers it. */
export type LogOption = { id: 'activity' | 'resource' | 'trip'; path: string; label: string };

/**
 * What "+ Log" offers on one object, most common first: an activity always, a fill or charge
 * when the object tracks a resource, a trip when it has a distance counter. The caller decides
 * `offersTrip` and `resourceCategory` with the same checks the entry form uses (see
 * routes/ObjectDetail.svelte), so the action never offers a category the form would refuse.
 */
export function logOptions(input: {
  oid: number;
  offersTrip: boolean;
  resourceCategory: string | null;
  labels: { activity: string; trip: string; resource: string };
}): LogOption[] {
  const base = `/objects/${input.oid}/activities/new`;
  const options: LogOption[] = [{ id: 'activity', path: base, label: input.labels.activity }];
  if (input.resourceCategory) options.push({ id: 'resource', path: `${base}?category=${input.resourceCategory}`, label: input.labels.resource });
  if (input.offersTrip) options.push({ id: 'trip', path: `${base}?category=trip`, label: input.labels.trip });
  return options;
}
