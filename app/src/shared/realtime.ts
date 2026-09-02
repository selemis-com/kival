import type { RealtimeMessage } from "kival-sdk";
import { browserTransport } from "./browserTransport";

/** Browser event emitted for every authorized realtime invalidation. */
export const KIVAL_REALTIME_EVENT = "kival:realtime";

/** Resolves the same-origin Kival realtime endpoint as a WebSocket URL. */
export function realtimeWebSocketUrl() {
  const url = new URL(browserTransport.url("/realtime"), window.location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

/** Publishes one invalidation to application features without coupling them to the socket owner. */
export function publishRealtimeMessage(message: RealtimeMessage) {
  window.dispatchEvent(
    new CustomEvent<RealtimeMessage>(KIVAL_REALTIME_EVENT, {
      detail: message,
    }),
  );
}
