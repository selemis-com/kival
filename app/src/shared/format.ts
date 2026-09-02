function dateTimeFormatter() {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

/** Formats a timestamp using the application's standard date and time presentation. */
export function formatTimestamp(value: string) {
  return dateTimeFormatter().format(new Date(value));
}

/** Formats a timestamp, returning a fallback when the value is not a valid date. */
export function formatTimestampOr(value: string, fallback: string) {
  const timestamp = new Date(value);

  if (Number.isNaN(timestamp.getTime())) {
    return fallback;
  }

  return dateTimeFormatter().format(timestamp);
}

/** Formats an event kind for display, optionally removing a namespace prefix first. */
export function formatEventKind(value: string, namespace?: string) {
  const prefix = namespace ? `${namespace}.` : null;
  const kind = prefix && value.startsWith(prefix) ? value.slice(prefix.length) : value;
  const words = namespace ? kind : kind.replaceAll(".", " ");

  return words.replaceAll("_", " ").replace(/\b\w/g, (character) => character.toUpperCase());
}
