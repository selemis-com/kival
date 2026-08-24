import type { KeyboardEvent } from "react";

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
