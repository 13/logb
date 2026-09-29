/**
 * `Intl` formatters, made once per locale and options and then reused.
 *
 * Constructing one resolves the locale and loads its data, which costs far more than the
 * `format` call itself -- and the dashboard, a timeline or the statistics screen format hundreds
 * of amounts, dates and counters per render, each of which used to build a fresh formatter. The
 * cache is keyed by the locale and the options as JSON, so two call sites asking for the same
 * thing share one. It only grows with the distinct combinations the code asks for (a handful per
 * locale), so it is never trimmed.
 *
 * A constructor that throws (an unknown currency code, say) throws on every call, exactly as
 * before: nothing is cached for it.
 */

function cached<O, F>(make: (locale: string | undefined, options?: O) => F) {
  const cache = new Map<string, F>();
  return (locale: string | undefined, options?: O): F => {
    const key = `${locale ?? ''}\u0000${options === undefined ? '' : JSON.stringify(options)}`;
    let f = cache.get(key);
    if (f === undefined) {
      f = make(locale, options);
      cache.set(key, f);
    }
    return f;
  };
}

export const numberFormat = cached<Intl.NumberFormatOptions, Intl.NumberFormat>(
  (l, o) => new Intl.NumberFormat(l, o),
);
export const dateTimeFormat = cached<Intl.DateTimeFormatOptions, Intl.DateTimeFormat>(
  (l, o) => new Intl.DateTimeFormat(l, o),
);
export const relativeTimeFormat = cached<Intl.RelativeTimeFormatOptions, Intl.RelativeTimeFormat>(
  (l, o) => new Intl.RelativeTimeFormat(l, o),
);
export const collator = cached<Intl.CollatorOptions, Intl.Collator>(
  (l, o) => new Intl.Collator(l, o),
);
