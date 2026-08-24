# Kival SDK

`kival-sdk` is the public Rust SDK for Kival. It provides Kival’s serializable wire types and an asynchronous HTTP client built on [`tower::Service`](https://docs.rs/tower/latest/tower/trait.Service.html).

The client classifies unsuccessful HTTP responses before they reach user-provided middleware. Timeouts, retries, tracing, metrics, and circuit breakers can therefore operate on structured `ClientError` values without reimplementing Kival’s response handling.

## Features

The `client` feature is enabled by default and includes:

* the asynchronous Reqwest client;
* the Tower transport stack;
* middleware and custom transport support.

Disable default features to use only Kival’s wire types:

```toml
[dependencies]
kival-sdk = { version = "0.1", default-features = false }
```

This is useful for servers, schema tooling, and other crates that serialize Kival data without making outbound requests.

## Creating a client

```rust,no_run
use kival_sdk::{ClientBuilder, ClientError, WorkspaceListParams};

async fn list_workspaces() -> Result<(), ClientError> {
    let client = ClientBuilder::new()
        .with_api_key(
            std::env::var("KIVAL_API_KEY")
                .expect("KIVAL_API_KEY is required"),
        )
        .connect("https://kival.example")?;

    let workspaces = client
        .list_workspaces(&WorkspaceListParams::default())
        .await?;

    println!("Found {} workspaces", workspaces.items.len());

    Ok(())
}
```

The server URL must be an HTTP or HTTPS origin root:

```text
https://kival.example
```

Path prefixes, embedded credentials, query strings, and fragments are rejected instead of being silently discarded.

## Middleware

Add Tower middleware with `ClientBuilder::layer`:

```rust,no_run
use std::time::Duration;

use kival_sdk::{ClientBuilder, ClientError};
use tower::timeout::TimeoutLayer;

fn main() -> Result<(), ClientError> {
    let client = ClientBuilder::new()
        .layer(TimeoutLayer::new(Duration::from_secs(10)))
        .connect("https://kival.example")?;

    Ok(())
}
```

Layer ordering follows [`tower::ServiceBuilder`](https://docs.rs/tower/latest/tower/struct.ServiceBuilder.html). The first layer added receives the request first.

For:

```rust,ignore
ClientBuilder::new()
    .layer(A)
    .layer(B)
```

requests flow through:

```text
A → B → transport
```

Responses and errors flow back through:

```text
transport → B → A
```

Middleware errors must be convertible into `ClientError`. Tower’s boxed errors are supported directly. Custom middleware can use `ClientError::transport` or implement `From<YourError> for ClientError`.

## Custom transports

`ClientBuilder::connect_with_transport` accepts any compatible Tower service over `reqwest::Request`.

Custom transports are useful for:

* deterministic tests;
* request recording;
* alternate HTTP executors;
* specialised transport instrumentation.

Use `KivalClient::boxed` to erase a concrete transport stack into `BoxTransport`. This is useful when clients with different middleware stacks must share a common type.

## Authentication

Pass an API key with `ClientBuilder::with_api_key`.

Authenticated operations reject missing credentials before executing the transport. Public health and readiness requests never include the configured API key.

Authorization headers are marked as sensitive in Reqwest, so their values are redacted from ordinary request debug output.

## Realtime invalidations

API keys with `realtime:read` may connect to Kival's authenticated realtime endpoint:

```text
wss://kival.example/api/v1/realtime
Authorization: Bearer <token>
```

Object-scoped messages additionally require `objects:read` or `objects:write`, the workspace must be delegated to the key, and the owning user must still be able to view the object. `events:read` remains an independent capability. Personal inbox invalidations are delivered only to interactive browser sessions.

The WebSocket carries lightweight invalidations. Fetch authoritative state through the HTTP client, assume messages may be delayed, duplicated, missed, or reordered, and resynchronize after reconnecting. Kival does not provide WebSocket replay.

The Rust SDK exports `RealtimeMessage` for decoding the wire payload. A WebSocket client used with an API key must send the bearer token in the handshake `Authorization` header.

## Errors

`ClientError::Api` preserves the information needed to inspect and classify a Kival API failure:

* the HTTP status;
* a semantic `ApiErrorKind`;
* Kival’s stable error code, when present;
* the human-readable error message;
* the raw `Retry-After` header, when present.

Error response bodies are bounded before being retained. Transport and middleware failures preserve their underlying source through `TransportError`.

## Attachments

The buffered attachment methods are convenient for ordinary payloads:

* `upload_object_attachment`
* `get_object_attachment_content`

For large payloads, use the streaming alternatives:

* `upload_object_attachment_body` accepts a `reqwest::Body`;
* `get_object_attachment_content_response` returns the response for incremental consumption.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in these crates by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
