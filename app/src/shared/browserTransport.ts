import { API_PREFIX, type KivalRequestInit, type KivalTransport } from "kival-sdk";
import { decodeBytesResponse, decodeJsonResponse, fetchResponse } from "kival-sdk/transports";

const CSRF_COOKIE_NAME = "__Host-kival_csrf";
const CSRF_HEADER_NAME = "x-csrf-token";

export const browserTransport: KivalTransport = {
  baseUrl: "",
  apiPrefix: API_PREFIX,

  url(path) {
    const normalizedPath = path.startsWith("/") ? path : `/${path}`;
    return `${API_PREFIX}${normalizedPath}`;
  },

  async requestResponse(path: string, init: KivalRequestInit = {}) {
    const { auth: _auth, ...requestInit } = init;
    const headers = new Headers(requestInit.headers);

    if (isUnsafeMethod(requestInit.method)) {
      const csrfToken = getCookie(CSRF_COOKIE_NAME);

      if (csrfToken) {
        headers.set(CSRF_HEADER_NAME, csrfToken);
      }
    }

    return fetchResponse(fetch, this.url(path), {
      ...requestInit,
      credentials: "include",
      headers,
    });
  },

  async requestJson<T>(path: string, init: KivalRequestInit = {}) {
    return decodeJsonResponse<T>(await this.requestResponse(path, init));
  },

  async requestBytes(path: string, init: KivalRequestInit = {}) {
    return decodeBytesResponse(await this.requestResponse(path, init));
  },

  async requestVoid(path: string, init: KivalRequestInit = {}) {
    const response = await this.requestResponse(path, init);
    await response.body?.cancel();
  },
};

function getCookie(name: string) {
  const prefix = `${name}=`;

  for (const part of document.cookie.split(";")) {
    const cookie = part.trim();

    if (cookie.startsWith(prefix)) {
      return cookie.slice(prefix.length);
    }
  }

  return null;
}

function isUnsafeMethod(method: string | undefined) {
  const normalizedMethod = (method ?? "GET").toUpperCase();
  return !["GET", "HEAD", "OPTIONS", "TRACE"].includes(normalizedMethod);
}
