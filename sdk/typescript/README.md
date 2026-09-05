# Kival SDK

`kival-sdk` is the public TypeScript SDK for Kival. It provides Kival’s serializable wire types and a typed HTTP client built on the standard [Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API).

The client classifies API, response-decoding, and transport failures before actions return.
Applications can therefore handle status codes, stable Kival error codes, malformed responses, and
request failures without reimplementing Kival’s transport boundary.

The package has no runtime dependencies.

## Installation

```bash
pnpm add kival-sdk
```

## Creating a client

Create a client with `createKivalClient`:

```typescript
import { createKivalClient } from "kival-sdk";

const apiKey = process.env.KIVAL_API_KEY;

if (!apiKey) {
  throw new Error("KIVAL_API_KEY is required");
}

const client = createKivalClient({
  baseUrl: "https://kival.example",
  apiKey,
});

const workspaces = await client.listWorkspaces();

console.log(`Found ${workspaces.items.length} workspaces`);
```

The client can also construct canonical browser links for resources on the same Kival instance:

```typescript
const url = client.objectUrl(workspaceId, objectId);
console.log(url);
// https://kival.example/w/<workspace-id>/objects/<object-id>
```

The server URL must be an HTTP or HTTPS origin root:

```text
https://kival.example
```

Path prefixes, embedded credentials, query strings, and fragments are rejected instead of being silently discarded.

The default API prefix is:

```text
/api/v1
```

`baseUrl` is prepended to this prefix. A custom prefix can be supplied when Kival is exposed through a different route:

```typescript
const client = createKivalClient({
  baseUrl: "https://kival.example",
  apiPrefix: "/internal/kival",
  apiKey,
});
```

The default HTTP transport applies a 30-second request timeout, matching the Rust SDK. Configure a
different positive timeout in milliseconds when creating the client:

```typescript
const client = createKivalClient({
  baseUrl: "https://kival.example",
  apiKey,
  timeout: 10_000,
});
```

Timeouts reject with `KivalTransportError` and `kind: "timeout"`. `requestJson` and `requestBytes`
apply the timeout through complete body consumption. `requestResponse` clears the SDK timeout once
response headers arrive; a caller-provided `AbortSignal` remains connected to its body stream.
Caller cancellation retains `kind: "abort"` when it fires before the SDK timeout. The transport
does not automatically retry requests.

## Actions

`createKivalClient` binds the complete API-key-capable action set to the client.

The SDK includes actions for:

* users and administrative user management;
* groups and group memberships;
* workspaces, memberships, and workspace groups;
* objects, versions, edges, backlinks, and grants;
* attachments;
* graph queries and search;
* object, workspace, and global events.

Actions accept one typed parameter object and return the corresponding Kival response types. Each
action exports named `*Parameters` and `*ReturnType` types for reuse in integrations:

```typescript
const object = await client.createObject({
  workspaceId,
  input: {
    title: "Architecture decision",
    body: "The system will use...",
  },
});

const events = await client.listObjectEvents({
  workspaceId,
  objectId: object.object.id,
  order: "desc",
  limit: 50,
});
```

Every network operation supports cancellation through its parameter object:

```typescript
import { KivalTransportError } from "kival-sdk";

const controller = new AbortController();

const update = client.updateObject({
  workspaceId,
  objectId,
  input: {
    expected_current_version_id: currentVersionId,
    title: "Updated title",
  },
  signal: controller.signal,
});

controller.abort();

try {
  await update;
} catch (error) {
  if (!(error instanceof KivalTransportError && error.kind === "abort")) throw error;
}
```

Aborting stops the client from waiting for the operation. It cannot guarantee that the server has
not already applied a mutation.

List responses expose their entries through `items` and may include a `next_cursor` for subsequent requests.

## Wire semantics

The exported TypeScript types mirror the Rust SDK’s canonical JSON wire types. Their JSDoc is
included in the generated declaration files and appears in editor hovers and completion details.

Collection queries use cursor pagination:

```typescript
let cursor: string | undefined;

do {
  const page = await client.listObjects({
    workspaceId,
    limit: 100,
    cursor,
  });

  consume(page.items);
  cursor = page.next_cursor;
} while (cursor);
```

Page limits default to 50 and are capped at 200. A missing `next_cursor` indicates the final page;
list responses do not include a total count.

For group and workspace descriptions, PATCH fields are tri-state:

* omit the property to leave it unchanged;
* pass `null` to clear it;
* pass a string to replace it.

Object updates must include `expected_current_version_id`; Kival returns a conflict if that version
is no longer current. Omitted `title`, `body`, and `metadata` values inherit from the current version,
and explicit `null` values are invalid. A semantic no-op keeps the existing current version. Metadata
is intentionally flat: each top-level value may be a JSON scalar or a one-dimensional array of JSON
scalars. Nested objects and nested arrays are rejected.

The `effective_role` returned for a workspace or object is derived from the authenticated user’s
authority. API-key scopes remain an additional restriction and can only reduce that authority.

## Standalone actions

Actions can also be imported independently from `kival-sdk/actions`.

This is useful when an application provides its own compatible request implementation or does not need the complete bound client:

```typescript
import { listWorkspaces } from "kival-sdk/actions";
import { http } from "kival-sdk/transports";

const transport = http({
  baseUrl: "https://kival.example",
  apiKey,
});

const workspaces = await listWorkspaces(transport);
```

Wire types are available from the package root and from `kival-sdk/types`:

```typescript
import type {
  ObjectResponse,
  SearchResponse,
  Workspace,
} from "kival-sdk";
```

## Authentication

### API keys

Pass an API key with the `apiKey` option:

```typescript
const client = createKivalClient({
  baseUrl: "https://kival.example",
  apiKey,
});
```

The client sends the key as a bearer token:

```text
Authorization: Bearer <token>
```

An API key is required when using the default HTTP transport. Empty API keys are rejected when the
client is created. The transport always forces `credentials: "omit"`; callers cannot enable cookie
authentication through individual request options.

Authenticated requests use the `apiKey` request policy by default. Public health and readiness
checks explicitly use `auth: "none"` and never send the configured key.

Interactive browser authentication, passkeys, sessions, API-key management, workspace creation, and personal state such as pins, favorites, notification preferences, and the inbox are intentionally not part of the public client. Those operations require a Kival browser session and belong to the Kival web application.

## Realtime invalidations

API keys with `realtime:read` may connect to the same realtime endpoint used by the Kival web application:

```text
wss://kival.example/api/v1/realtime
Authorization: Bearer <token>
```

Object-scoped messages additionally require `objects:read` or `objects:write`, the workspace must be delegated to the key, and the owning user must still be able to view the object. `events:read` is independent and is not required for realtime delivery. Personal `inbox.updated` messages are reserved for interactive browser sessions.

Realtime messages are lightweight invalidations rather than authoritative resource state. They may be delayed, duplicated, missed, or received out of order. After reconnecting, refresh the HTTP-backed state your integration maintains. The WebSocket is not a replayable event log.

The standard browser `WebSocket` constructor cannot attach an `Authorization` header. API-key consumers therefore need a server/runtime WebSocket client that supports custom handshake headers. Kival does not place API keys in WebSocket query strings.

## Search

`searchWorkspace` searches visible authored content in a workspace. Search is scoped to each
object's current immutable version by default; set `include_history` to `true` to search previous
versions as well. Its `categories` option accepts a comma-separated list containing `title`, `body`,
or `metadata`. Categories select complete indexed version values; nested metadata paths such as
`metadata.kind` are not supported.

Search modes have distinct matching behavior:

* `auto` combines normalized full-text matching with literal and exact checks and adds lower-ranked
  partial-term matches for plain multi-word queries;
* `text` uses normalized tokens and PostgreSQL web-search syntax;
* `literal` matches one contiguous substring;
* `exact` matches the complete stored category value.

Literal and exact matching can be case-sensitive. Full-text matching remains case-insensitive.
The `context` option changes snippet length without changing which values match. Search results include the matched version metadata and object lifecycle status for lightweight
triage. Plain multi-word `auto` results also include `term_coverage`, which reports the matched
query terms and total query-term count so clients can distinguish full and partial matches without
inferring from rank. Results are cursor-paginated; pass `next_cursor` back as `cursor` with the same
query and filters to continue.

## Custom fetch implementations

Provide a custom `fetch` implementation for testing, request recording, alternate runtimes, or transport instrumentation:

```typescript
import { createKivalClient } from "kival-sdk";

const client = createKivalClient({
  baseUrl: "https://kival.example",
  apiKey,
  fetch: async (input, init) => {
    console.debug(init?.method ?? "GET", input);

    return fetch(input, init);
  },
});
```

The implementation must have the same interface as the standard `fetch` function.

## Custom transports

For complete control over request execution, pass a `KivalTransport`:

```typescript
import {
  createKivalClient,
  decodeBytesResponse,
  decodeJsonResponse,
  fetchResponse,
  type KivalTransport,
} from "kival-sdk";

const transport: KivalTransport = {
  baseUrl: "https://kival.example",
  apiPrefix: "/api/v1",

  url(path) {
    return `${this.baseUrl}${this.apiPrefix}${path}`;
  },

  async requestJson<T>(path, init) {
    return decodeJsonResponse<T>(await this.requestResponse(path, init));
  },

  async requestBytes(path, init) {
    return decodeBytesResponse(await this.requestResponse(path, init));
  },

  async requestVoid(path, init) {
    const response = await this.requestResponse(path, init);
    await response.body?.cancel();
  },

  async requestResponse(path, init = {}) {
    const { auth = "apiKey", ...requestInit } = init;
    const headers = new Headers(requestInit.headers);

    if (auth === "apiKey") {
      headers.set("authorization", `Bearer ${apiKey}`);
    } else {
      headers.delete("authorization");
    }

    return fetchResponse(customFetch, this.url(path), {
      ...requestInit,
      headers,
    });
  },
};

const client = createKivalClient({ transport });
```

A transport provides:

* `requestJson`, which requires and decodes a JSON response;
* `requestBytes`, which buffers the complete response body as a `Uint8Array`;
* `requestVoid`, which executes an operation whose response body is intentionally ignored;
* `requestResponse`, which returns the raw Fetch API response once headers arrive;
* `url`, which constructs a complete API URL;
* `baseUrl`;
* `apiPrefix`.

Request options include an `auth` policy. It defaults to `apiKey`; custom transports must honor
`auth: "none"` without forwarding an authorization credential. The policy is SDK metadata and
must be removed before passing the options to a Fetch implementation.

Custom transports are useful for:

* deterministic tests;
* request recording;
* alternate HTTP executors;
* specialized transport instrumentation;
* integration with an existing application request layer.

## Errors

Unsuccessful HTTP responses throw `KivalApiError`:

```typescript
import {
  createKivalClient,
  KivalApiError,
  KivalResponseError,
  KivalTransportError,
} from "kival-sdk";

const client = createKivalClient({
  baseUrl: "https://kival.example",
  apiKey,
});

try {
  await client.getWorkspace({
    workspaceId: "01900000-0000-7000-8000-000000000000",
  });
} catch (error) {
  if (error instanceof KivalApiError) {
    console.error("Status:", error.status, error.statusText);
    console.error("Kind:", error.kind);
    console.error("Code:", error.code);
    console.error("Retry after:", error.retryAfterMilliseconds);
    console.error("Request ID:", error.headers.get("x-request-id"));
    console.error("Body:", error.body);
  } else if (error instanceof KivalResponseError) {
    console.error("Response decoding failed:", error.response.status, error.cause);
  } else if (error instanceof KivalTransportError) {
    console.error("Transport failed:", error.kind, error.cause);
  } else {
    throw error;
  }
}
```

`KivalApiError` preserves:

* the HTTP status and status text;
* a semantic `kind`: `unauthorized`, `forbidden`, `notFound`, `conflict`, `rateLimited`,
  `invalidRequest`, `serverError`, or `other`;
* the response headers;
* the raw `Retry-After` value and, for valid delta-seconds values, its
  `retryAfterMilliseconds` conversion;
* Kival’s stable error code, when present;
* the human-readable error message;
* parsed JSON or the original plain-text response body, bounded to 64 KiB.

HTTP-date `Retry-After` values remain available through `retryAfter` but are not converted to a
delay.

`KivalResponseError` reports successful responses that violate an action’s decoding contract, such
as an empty body or malformed JSON where JSON is required. Its `kind` is always `decode`.

`KivalTransportError` classifies request and response-body transport failures as `connect`,
`timeout`, `abort`, or `other`. The original failure remains available through `cause`. Raw response
streams report failures through the runtime's Fetch API after `requestResponse` resolves.

## Attachments

`uploadObjectAttachment` accepts the runtime’s native `BodyInit` values. Portable choices include
strings, blobs, files, array buffers, typed-array views, URL search parameters, and form data:

```typescript
const attachment = await client.uploadObjectAttachment({
  workspaceId,
  objectId,
  params: {
    name: file.name,
    media_type: file.type,
    metadata: JSON.stringify({ source: "browser" }),
  },
  body: file,
});
```

For `ReadableStream` request bodies, the SDK supplies `duplex: "half"` automatically for Fetch
implementations such as Node.js that require it for streaming uploads.

`getObjectAttachmentContent` fetches content and buffers the body as a `Uint8Array`.
`getObjectAttachmentContentResponse` returns the authenticated Fetch API `Response` for streaming
or runtime-specific download handling:

```typescript
const response = await client.getObjectAttachmentContentResponse({
  workspaceId,
  objectId,
  attachmentId: attachment.id,
});

await streamToDestination(response.body);
```

The SDK does not expose a bare attachment-content URL because browser navigation and HTML elements
cannot attach the API-key authorization header.

## Package exports

The complete SDK is available from:

```typescript
import { createKivalClient } from "kival-sdk";
```

Individual modules can also be imported through:

```text
kival-sdk/actions
kival-sdk/clients
kival-sdk/errors
kival-sdk/transports
kival-sdk/types
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this package by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
