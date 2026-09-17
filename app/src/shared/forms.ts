import type { KeyboardEvent } from "react";

type SoleResultOptions = {
  enabled?: boolean;
  hasMore?: boolean;
};

/** Focuses an existing text value for continued typing without selecting or replacing it. */
export function focusTextControlAtEnd(control: HTMLInputElement | HTMLTextAreaElement | null) {
  if (!control) {
    return;
  }

  const end = control.value.length;
  control.focus();
  control.setSelectionRange(end, end);
}

/** Selects the only complete result on Enter for searchable picker and directory inputs. */
export function selectSoleResultOnEnter<T>(
  event: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>,
  results: readonly T[],
  onSelect: (result: T) => void,
  { enabled = true, hasMore = false }: SoleResultOptions = {},
) {
  if (
    !enabled ||
    hasMore ||
    results.length !== 1 ||
    event.key !== "Enter" ||
    event.repeat ||
    event.defaultPrevented ||
    event.nativeEvent.isComposing
  ) {
    return false;
  }

  event.preventDefault();
  onSelect(results[0]);
  return true;
}

/** Submits a textarea's owning form on Enter while preserving Shift+Enter newlines. */
export function submitFormOnEnter(event: KeyboardEvent<HTMLTextAreaElement>) {
  if (
    event.key !== "Enter" ||
    event.shiftKey ||
    event.repeat ||
    event.defaultPrevented ||
    event.nativeEvent.isComposing
  ) {
    return;
  }

  const form = event.currentTarget.form;
  if (!form) {
    return;
  }

  event.preventDefault();
  form.requestSubmit();
}
