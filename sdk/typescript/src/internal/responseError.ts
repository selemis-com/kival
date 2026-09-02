import { KivalApiError } from "../errors/index.js";

const MAX_ERROR_BODY_BYTES = 64 * 1024;
const TRUNCATED_SUFFIX = " [response body truncated]";

/** Converts an unsuccessful Fetch API response into a structured Kival API error. */
export async function getResponseError(response: Response) {
  const { text, truncated } = await readLimitedText(response);
  let body: unknown = null;
  let message = `Request failed: ${response.status}`;
  let code: string | null = null;

  if (text) {
    body = text;
    message = text.trim() || message;

    try {
      body = JSON.parse(text) as unknown;
    } catch {
      return apiError(response, withTruncation(message, truncated), code, body);
    }
  }

  if (typeof body === "string") {
    message = body;
  } else if (isRecord(body)) {
    const error = body.error;

    if (isRecord(error)) {
      if (typeof error.message === "string") {
        message = error.message;
      }

      if (typeof error.code === "string") {
        code = error.code;
      }
    } else if (typeof error === "string") {
      message = error;
    } else if (typeof body.message === "string") {
      message = body.message;
    }
  }

  return apiError(response, withTruncation(message, truncated), code, body);
}

function apiError(response: Response, message: string, code: string | null, body: unknown) {
  return new KivalApiError(
    response.status,
    message,
    code,
    body,
    response.statusText,
    response.headers,
  );
}

function withTruncation(message: string, truncated: boolean) {
  return truncated ? `${message}${TRUNCATED_SUFFIX}` : message;
}

async function readLimitedText(response: Response) {
  if (!response.body) return { text: "", truncated: false };

  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  let truncated = false;

  while (length < MAX_ERROR_BODY_BYTES) {
    const { done, value } = await reader.read();
    if (done) break;

    const remaining = MAX_ERROR_BODY_BYTES - length;
    if (value.byteLength > remaining) {
      chunks.push(value.subarray(0, remaining));
      length += remaining;
      truncated = true;
      await reader.cancel();
      break;
    }

    chunks.push(value);
    length += value.byteLength;

    if (length === MAX_ERROR_BODY_BYTES) {
      const next = await reader.read();
      truncated = !next.done;
      if (truncated) await reader.cancel();
      break;
    }
  }

  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }

  return { text: new TextDecoder().decode(bytes), truncated };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
